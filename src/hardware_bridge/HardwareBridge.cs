// In-process bridge to LibreHardwareMonitorLib.dll, same role as the original
// OpenHardwareMonitorApi.dll. TrafficMonitorBar hosts the CLR and calls
// TrafficMonitorBar.Hardware.Bridge.Dispatch; there is no helper process.
//
// Dispatch argument, LF-separated:
//   <native utf-8 buffer pointer>
//   <capacity>
//   <init|update|close>
//   <payload>
// init payload is cpu, gpu, hdd, mbd (each 0 or 1) and the LibreHardwareMonitorLib.dll
// path, separated by tabs. Dispatch writes one JSON object into the buffer and returns 0.

using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;

namespace TrafficMonitorBar.Hardware;

public static class Bridge
{
    private static readonly object Gate = new();
    private static object computer;
    private static string probeDir;
    private static bool hooked;

    public static int Dispatch(string arg)
    {
        lock (Gate)
        {
            if (!Parse(arg, out nint ptr, out int cap, out string command, out string payload))
                return 1;
            try
            {
                EnsureHook();
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
                try { return Write(ptr, cap, Error(Unwrap(ex).Message)); }
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

    private static void EnsureHook()
    {
        if (hooked)
            return;
        hooked = true;
        AppDomain.CurrentDomain.AssemblyResolve += Resolve;
    }

    private static Assembly Resolve(object sender, ResolveEventArgs e)
    {
        string file = new AssemblyName(e.Name).Name + ".dll";
        if (!string.IsNullOrEmpty(probeDir))
        {
            string candidate = Path.Combine(probeDir, file);
            if (File.Exists(candidate))
                return Assembly.LoadFrom(candidate);
        }
        string dir = Path.GetDirectoryName(typeof(Bridge).Assembly.Location);
        if (string.IsNullOrEmpty(dir))
            return null;
        string local = Path.Combine(dir, file);
        return File.Exists(local) ? Assembly.LoadFrom(local) : null;
    }

    private static string Init(string payload)
    {
        CloseMonitor();
        string[] parts = (payload ?? string.Empty).Split(['\t'], 5);
        if (parts.Length < 5)
            return Error("invalid init request");
        string dllPath = parts[4];
        try
        {
            if (!File.Exists(dllPath))
                return Error("LibreHardwareMonitorLib.dll not found");
            probeDir = Path.GetDirectoryName(Path.GetFullPath(dllPath));
            Assembly lhm = Assembly.LoadFrom(dllPath);
            Type computerType = lhm.GetType("LibreHardwareMonitor.Hardware.Computer", true);
            computer = Activator.CreateInstance(computerType);
            SetProp(computer, "IsCpuEnabled", parts[0] == "1");
            SetProp(computer, "IsGpuEnabled", parts[1] == "1");
            SetProp(computer, "IsStorageEnabled", parts[2] == "1");
            SetProp(computer, "IsMotherboardEnabled", parts[3] == "1");
            try
            {
                computerType.GetMethod("Open", Type.EmptyTypes).Invoke(computer, null);
            }
            catch (Exception ex)
            {
                CloseMonitor();
                return Error(Unwrap(ex).Message);
            }
            return "{}";
        }
        catch (Exception ex)
        {
            CloseMonitor();
            return Error(Unwrap(ex).Message);
        }
    }

    private static void CloseMonitor()
    {
        if (computer == null)
            return;
        try { computer.GetType().GetMethod("Close", Type.EmptyTypes).Invoke(computer, null); }
        catch { }
        computer = null;
    }

    private static string Error(string message)
    {
        return "{\"error\":" + Quote(message) + "}";
    }

    private static Exception Unwrap(Exception ex)
    {
        while (ex is TargetInvocationException && ex.InnerException != null)
            ex = ex.InnerException;
        return ex;
    }

    private static void SetProp(object o, string name, bool value)
    {
        PropertyInfo p = o.GetType().GetProperty(name);
        p?.SetValue(o, value, null);
    }

    private static object GetProp(object o, string name)
    {
        PropertyInfo p = o.GetType().GetProperty(name);
        if (p == null)
        {
            foreach (Type itf in o.GetType().GetInterfaces())
            {
                p = itf.GetProperty(name);
                if (p != null) break;
            }
        }
        return p?.GetValue(o, null);
    }

    private static IEnumerable<object> Items(object list)
    {
        if (list is not System.Collections.IEnumerable e) yield break;
        foreach (object o in e) yield return o;
    }

    private static string Str(object o)
    {
        return o == null ? string.Empty : o.ToString();
    }

    private static void Update(object hw)
    {
        MethodInfo m = hw.GetType().GetMethod("Update", Type.EmptyTypes);
        if (m == null)
        {
            foreach (Type itf in hw.GetType().GetInterfaces())
            {
                m = itf.GetMethod("Update", Type.EmptyTypes);
                if (m != null) break;
            }
        }
        m?.Invoke(hw, null);
        foreach (object sub in Items(GetProp(hw, "SubHardware")))
            Update(sub);
    }

    private static float SensorValue(object sensor)
    {
        object v = GetProp(sensor, "Value");
        if (v == null) return -1;
        return Convert.ToSingle(v, CultureInfo.InvariantCulture);
    }

    private class Sensor
    {
        public string Type;
        public string Name;
        public float Value;
    }

    private static List<Sensor> Sensors(object hw)
    {
        var list = new List<Sensor>();
        foreach (object s in Items(GetProp(hw, "Sensors")))
        {
            var item = new Sensor
            {
                Type = Str(GetProp(s, "SensorType")),
                Name = Str(GetProp(s, "Name")),
                Value = SensorValue(s)
            };
            list.Add(item);
        }
        return list;
    }

    // COpenHardwareMonitor::GetHardwareTemperature
    private static float HardwareTemperature(object hw)
    {
        string type = Str(GetProp(hw, "HardwareType"));
        string preferred = type == "Cpu" ? "Core Average" : (type.StartsWith("Gpu") ? "GPU Core" : null);
        var temps = new List<float>();
        float preferredValue = -1;
        foreach (Sensor s in Sensors(hw))
        {
            if (s.Type != "Temperature" || s.Value < 0) continue;
            temps.Add(s.Value);
            if (preferred != null && s.Name == preferred) preferredValue = s.Value;
        }
        if (preferredValue >= 0) return preferredValue;
        if (temps.Count > 0)
        {
            float sum = 0;
            foreach (float t in temps) sum += t;
            return sum / temps.Count;
        }
        foreach (object sub in Items(GetProp(hw, "SubHardware")))
        {
            float t = HardwareTemperature(sub);
            if (t >= 0) return t;
        }
        return -1;
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

#pragma warning disable IDE0059 // Unnecessary assignment of a value
        float cpuTemp = -1, cpuFreq = -1, gpuTemp = -1, gpuUsage = -1, mbdTemp = -1;
#pragma warning restore IDE0059 // Unnecessary assignment of a value
        float nvTemp = -1, amdTemp = -1, intelTemp = -1, nvUsage = -1, amdUsage = -1, intelUsage = -1;
        var cpuTemps = new Dictionary<string, float>();
        var hddTemps = new Dictionary<string, float>();
        var hddUsage = new Dictionary<string, float>();

        foreach (object hw in Items(GetProp(computer, "Hardware")))
        {
            Update(hw);
            string type = Str(GetProp(hw, "HardwareType"));
            string name = Str(GetProp(hw, "Name"));
            List<Sensor> sensors = Sensors(hw);
            switch (type)
            {
                case "Cpu":
                    if (cpuTemp < 0)
                    {
                        cpuTemps.Clear();
                        float sum = 0; int count = 0;
                        foreach (Sensor s in sensors)
                        {
                            if (s.Type != "Temperature" || s.Name.Contains("Distance to TjMax")) continue;
                            cpuTemps[s.Name] = s.Value;
                            if (s.Value > 0) { sum += s.Value; count++; }
                        }
                        if (count > 0) cpuTemp = sum / count;
                    }
                    if (cpuFreq < 0)
                    {
                        float sum = 0; int count = 0;
                        foreach (Sensor s in sensors)
                            if (s.Type == "Clock" && s.Name != "Bus Speed" && s.Value > 0) { sum += s.Value; count++; }
                        if (count > 0) cpuFreq = sum / count / 1000f;
                    }
                    break;

                case "GpuNvidia":
                case "GpuAmd":
                case "GpuIntel":
                    {
                        float t = HardwareTemperature(hw);
                        float u = -1, max = 0;
                        foreach (Sensor s in sensors)
                        {
                            if (s.Type != "Load") continue;
                            if (s.Name == "GPU Core") { u = s.Value; break; }
                            if (s.Value > max) max = s.Value;
                        }
                        if (u < 0) u = max;
                        if (type == "GpuNvidia") { if (nvTemp < 0) nvTemp = t; if (nvUsage < 0) nvUsage = u; }
                        else if (type == "GpuAmd") { if (amdTemp < 0) amdTemp = t; if (amdUsage < 0) amdUsage = u; }
                        else { if (intelTemp < 0) intelTemp = t; if (intelUsage < 0) intelUsage = u; }
                        break;
                    }
                case "Storage":
                    {
                        InsertUnique(hddTemps, name, HardwareTemperature(hw));
                        float u = -1;
                        foreach (Sensor s in sensors)
                            if (s.Type == "Load" && s.Name == "Total Activity") { u = s.Value; break; }
                        InsertUnique(hddUsage, name, u);
                        break;
                    }
                case "Motherboard":
                    if (mbdTemp < 0) mbdTemp = HardwareTemperature(hw);
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
