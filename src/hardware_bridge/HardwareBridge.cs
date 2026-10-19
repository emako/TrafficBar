// In-process hardware bridge. TrafficMonitorBar hosts the CLR and calls
// TrafficMonitorBar.Hardware.Bridge.Dispatch. LibreHardwareMonitor is a
// compile-time reference of this assembly, so there is no separate monitor DLL.
//
// Dispatch argument, LF-separated:
//   <native utf-8 buffer pointer>
//   <capacity>
//   <init|update|close>
//   <payload>
// init payload is cpu, gpu, hdd, mbd (each 0 or 1), separated by tabs.
// Dispatch writes one JSON object into the buffer and returns 0.

using System.Globalization;
using System.Runtime.InteropServices;
using System.ServiceProcess;
using System.Text;
using LibreHardwareMonitor.Hardware;

namespace TrafficMonitorBar.Hardware;

public static class Bridge
{
    private static readonly object Gate = new();
    private static Computer computer;

    public static int Dispatch(string arg)
    {
        lock (Gate)
        {
            if (!Parse(arg, out nint ptr, out int cap, out string command, out string payload))
                return 1;
            try
            {
                string json;
                if (command == "init")
                    json = Init(payload);
                else if (command == "update")
                    json = Collect();
                else if (command == "close")
                {
                    CloseMonitor();
                    json = "{}";
                }
                else
                    json = Error("unknown command");
                return Write(ptr, cap, json);
            }
            catch (Exception ex)
            {
                try { return Write(ptr, cap, Error(ex.Message)); }
                catch { return 1; }
            }
        }
    }

    private static bool Parse(string arg, out nint ptr, out int cap, out string command, out string payload)
    {
        ptr = IntPtr.Zero;
        cap = 0;
        command = string.Empty;
        payload = string.Empty;
        if (string.IsNullOrEmpty(arg))
            return false;
        string[] lines = arg.Split(['\n'], 4);
        if (lines.Length < 3)
            return false;
        if (!ulong.TryParse(lines[0].Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out ulong addr))
            return false;
        if (!int.TryParse(lines[1].Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out cap) || cap <= 1)
            return false;
        ptr = (nint)addr;
        if (ptr == IntPtr.Zero)
            return false;
        command = lines[2].Trim();
        payload = lines.Length > 3 ? lines[3] : string.Empty;
        return true;
    }

    private static int Write(nint ptr, int cap, string text)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(text ?? string.Empty);
        if (bytes.Length + 1 > cap)
            return 2;
        Marshal.Copy(bytes, 0, ptr, bytes.Length);
        Marshal.WriteByte(ptr, bytes.Length, 0);
        return 0;
    }

    private static string Init(string payload)
    {
        CloseMonitor();
        string[] parts = (payload ?? string.Empty).Split('\t');
        if (parts.Length < 4)
            return Error("invalid init request");
        EnsurePawnIo();
        var monitor = new Computer
        {
            IsCpuEnabled = parts[0] == "1",
            IsGpuEnabled = parts[1] == "1",
            IsStorageEnabled = parts[2] == "1",
            IsMotherboardEnabled = parts[3] == "1",
        };
        try
        {
            monitor.Open();
        }
        catch (Exception ex)
        {
            try { monitor.Close(); } catch { }
            return Error(ex.Message);
        }
        computer = monitor;
        return "{}";
    }

    // LibreHardwareMonitor 0.9 reads CPU core temperature through the PawnIO
    // driver. The service is installed as manual and does not start itself.
    private static void EnsurePawnIo()
    {
        try
        {
            using (var service = new ServiceController("PawnIO"))
            {
                ServiceControllerStatus status = service.Status;
                if (status == ServiceControllerStatus.StartPending)
                {
                    service.WaitForStatus(ServiceControllerStatus.Running, TimeSpan.FromSeconds(8));
                    return;
                }
                if (status == ServiceControllerStatus.StopPending)
                {
                    service.WaitForStatus(ServiceControllerStatus.Stopped, TimeSpan.FromSeconds(8));
                    status = service.Status;
                }
                if (status == ServiceControllerStatus.Stopped)
                {
                    service.Start();
                    service.WaitForStatus(ServiceControllerStatus.Running, TimeSpan.FromSeconds(8));
                }
            }
        }
        catch
        {
        }
    }

    private static void CloseMonitor()
    {
        if (computer == null)
            return;
        try { computer.Close(); }
        catch { }
        computer = null;
    }

    private static string Error(string message)
    {
        return "{\"error\":" + Quote(message) + "}";
    }

    private static float SensorValue(ISensor sensor)
    {
        return sensor.Value ?? -1f;
    }

    // COpenHardwareMonitor::GetHardwareTemperature
    private static float HardwareTemperature(IHardware hw)
    {
        string preferred = hw.HardwareType == HardwareType.Cpu
            ? "Core Average"
            : (IsGpu(hw.HardwareType) ? "GPU Core" : null);
        var temps = new List<float>();
        float preferredValue = -1;
        foreach (ISensor sensor in hw.Sensors)
        {
            if (sensor.SensorType != SensorType.Temperature)
                continue;
            float value = SensorValue(sensor);
            if (value < 0)
                continue;
            temps.Add(value);
            if (preferred != null && sensor.Name == preferred)
                preferredValue = value;
        }
        if (preferredValue >= 0)
            return preferredValue;
        if (temps.Count > 0)
        {
            float sum = 0;
            foreach (float t in temps)
                sum += t;
            return sum / temps.Count;
        }
        foreach (IHardware sub in hw.SubHardware)
        {
            float t = HardwareTemperature(sub);
            if (t >= 0)
                return t;
        }
        return -1;
    }

    private static bool IsGpu(HardwareType type)
    {
        return type == HardwareType.GpuNvidia || type == HardwareType.GpuAmd || type == HardwareType.GpuIntel;
    }

    private static void Update(IHardware hw)
    {
        hw.Update();
        foreach (IHardware sub in hw.SubHardware)
            Update(sub);
    }

    private static void InsertUnique(Dictionary<string, float> map, string key, float value)
    {
        if (!map.ContainsKey(key)) { map[key] = value; return; }
        int n = 1;
        while (map.ContainsKey(key + " #" + n)) n++;
        map[key + " #" + n] = value;
    }

    private static string Collect()
    {
        if (computer == null)
            return Error("hardware monitor is not open");

        float cpuTemp = -1, cpuFreq = -1, gpuTemp = -1, gpuUsage = -1, mbdTemp = -1;
        float nvTemp = -1, amdTemp = -1, intelTemp = -1, nvUsage = -1, amdUsage = -1, intelUsage = -1;
        var cpuTemps = new Dictionary<string, float>();
        var hddTemps = new Dictionary<string, float>();
        var hddUsage = new Dictionary<string, float>();

        foreach (IHardware hw in computer.Hardware)
        {
            Update(hw);
            switch (hw.HardwareType)
            {
                case HardwareType.Cpu:
                    if (cpuTemp < 0)
                    {
                        cpuTemps.Clear();
                        float sum = 0; int count = 0;
                        foreach (ISensor sensor in hw.Sensors)
                        {
                            if (sensor.SensorType != SensorType.Temperature || sensor.Name.Contains("Distance to TjMax"))
                                continue;
                            float value = SensorValue(sensor);
                            cpuTemps[sensor.Name] = value;
                            if (value > 0) { sum += value; count++; }
                        }
                        if (count > 0)
                            cpuTemp = sum / count;
                    }
                    if (cpuFreq < 0)
                    {
                        float sum = 0; int count = 0;
                        foreach (ISensor sensor in hw.Sensors)
                        {
                            float value = SensorValue(sensor);
                            if (sensor.SensorType == SensorType.Clock && sensor.Name != "Bus Speed" && value > 0)
                            {
                                sum += value;
                                count++;
                            }
                        }
                        if (count > 0)
                            cpuFreq = sum / count / 1000f;
                    }
                    break;

                case HardwareType.GpuNvidia:
                case HardwareType.GpuAmd:
                case HardwareType.GpuIntel:
                    {
                        float t = HardwareTemperature(hw);
                        float u = -1, max = 0;
                        foreach (ISensor sensor in hw.Sensors)
                        {
                            if (sensor.SensorType != SensorType.Load)
                                continue;
                            float value = SensorValue(sensor);
                            if (sensor.Name == "GPU Core") { u = value; break; }
                            if (value > max)
                                max = value;
                        }
                        if (u < 0)
                            u = max;
                        if (hw.HardwareType == HardwareType.GpuNvidia) { if (nvTemp < 0) nvTemp = t; if (nvUsage < 0) nvUsage = u; }
                        else if (hw.HardwareType == HardwareType.GpuAmd) { if (amdTemp < 0) amdTemp = t; if (amdUsage < 0) amdUsage = u; }
                        else { if (intelTemp < 0) intelTemp = t; if (intelUsage < 0) intelUsage = u; }
                        break;
                    }
                case HardwareType.Storage:
                    {
                        InsertUnique(hddTemps, hw.Name, HardwareTemperature(hw));
                        float u = -1;
                        foreach (ISensor sensor in hw.Sensors)
                        {
                            if (sensor.SensorType == SensorType.Load && sensor.Name == "Total Activity")
                            {
                                u = SensorValue(sensor);
                                break;
                            }
                        }
                        InsertUnique(hddUsage, hw.Name, u);
                        break;
                    }
                case HardwareType.Motherboard:
                    if (mbdTemp < 0)
                        mbdTemp = HardwareTemperature(hw);
                    break;
            }
        }
        gpuTemp = nvTemp >= 0 ? nvTemp : (amdTemp >= 0 ? amdTemp : intelTemp);
        gpuUsage = nvUsage >= 0 ? nvUsage : (amdUsage >= 0 ? amdUsage : intelUsage);

        var sb = new StringBuilder();
        sb.Append('{');
        sb.Append("\"cpu_temp\":").Append(Num(cpuTemp));
        sb.Append(",\"cpu_freq\":").Append(Num(cpuFreq));
        sb.Append(",\"gpu_temp\":").Append(Num(gpuTemp));
        sb.Append(",\"gpu_usage\":").Append(Num(gpuUsage));
        sb.Append(",\"mbd_temp\":").Append(Num(mbdTemp));
        sb.Append(",\"cpu_temps\":").Append(Map(cpuTemps));
        sb.Append(",\"hdd_temps\":").Append(Map(hddTemps));
        sb.Append(",\"hdd_usage\":").Append(Map(hddUsage));
        sb.Append('}');
        return sb.ToString();
    }

    private static string Num(float v)
    {
        if (float.IsNaN(v) || float.IsInfinity(v)) v = -1;
        return v.ToString("0.###", CultureInfo.InvariantCulture);
    }

    private static string Map(Dictionary<string, float> map)
    {
        var sb = new StringBuilder("{");
        bool first = true;
        foreach (var kv in map)
        {
            if (!first) sb.Append(',');
            first = false;
            sb.Append(Quote(kv.Key)).Append(':').Append(Num(kv.Value));
        }
        sb.Append('}');
        return sb.ToString();
    }

    private static string Quote(string s)
    {
        var sb = new StringBuilder("\"");
        foreach (char c in s ?? string.Empty)
        {
            switch (c)
            {
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                case '\t': sb.Append("\\t"); break;
                default:
                    if (c < 0x20) sb.Append("\\u").Append(((int)c).ToString("x4"));
                    else sb.Append(c);
                    break;
            }
        }
        sb.Append('"');
        return sb.ToString();
    }
}
