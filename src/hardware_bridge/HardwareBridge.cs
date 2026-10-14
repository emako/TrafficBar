// Out-of-process bridge to LibreHardwareMonitorLib.dll (same library and sensor rules as the
// original OpenHardwareMonitorApi). Compiled with the C# 5 compiler that ships with .NET Framework.
//
// Protocol: one command per stdin line.
//   update  -> one JSON object line on stdout
//   exit    -> quit
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Text;

namespace TrafficMonitorBar.Hardware
{
    internal static class Program
    {
        private static object computer;
        private static Type hardwareTypeEnum;
        private static Type sensorTypeEnum;

        private static int Main(string[] args)
        {
            bool cpu = false, gpu = false, hdd = false, mbd = false;
            string dllPath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "LibreHardwareMonitorLib.dll");
            for (int i = 0; i < args.Length; i++)
            {
                switch (args[i])
                {
                    case "--cpu": cpu = true; break;
                    case "--gpu": gpu = true; break;
                    case "--hdd": hdd = true; break;
                    case "--mbd": mbd = true; break;
                    case "--lhm":
                        if (i + 1 < args.Length) dllPath = args[++i];
                        break;
                }
            }

            Console.OutputEncoding = new UTF8Encoding(false);
            var stdin = new StreamReader(Console.OpenStandardInput(), Encoding.UTF8);

            string initError = null;
            try
            {
                if (!File.Exists(dllPath))
                    throw new FileNotFoundException("LibreHardwareMonitorLib.dll not found", dllPath);
                string dir = Path.GetDirectoryName(Path.GetFullPath(dllPath));
                AppDomain.CurrentDomain.AssemblyResolve += delegate (object sender, ResolveEventArgs e)
                {
                    string candidate = Path.Combine(dir, new AssemblyName(e.Name).Name + ".dll");
                    return File.Exists(candidate) ? Assembly.LoadFrom(candidate) : null;
                };
                Assembly lhm = Assembly.LoadFrom(dllPath);
                Type computerType = lhm.GetType("LibreHardwareMonitor.Hardware.Computer", true);
                hardwareTypeEnum = lhm.GetType("LibreHardwareMonitor.Hardware.HardwareType", true);
                sensorTypeEnum = lhm.GetType("LibreHardwareMonitor.Hardware.SensorType", true);
                computer = Activator.CreateInstance(computerType);
                SetProp(computer, "IsCpuEnabled", cpu);
                SetProp(computer, "IsGpuEnabled", gpu);
                SetProp(computer, "IsStorageEnabled", hdd);
                SetProp(computer, "IsMotherboardEnabled", mbd);
                computerType.GetMethod("Open", Type.EmptyTypes).Invoke(computer, null);
            }
            catch (Exception ex)
            {
                initError = Unwrap(ex).Message;
            }

            string line;
            while ((line = stdin.ReadLine()) != null)
            {
                line = line.Trim();
                if (line == "exit")
                    break;
                if (line != "update")
                    continue;
                string json;
                if (initError != null)
                {
                    json = "{\"error\":" + Quote(initError) + "}";
                }
                else
                {
                    try { json = Collect(); }
                    catch (Exception ex) { json = "{\"error\":" + Quote(Unwrap(ex).Message) + "}"; }
                }
                Console.Out.WriteLine(json);
                Console.Out.Flush();
            }

            if (computer != null)
            {
                try { computer.GetType().GetMethod("Close", Type.EmptyTypes).Invoke(computer, null); }
                catch { }
            }
            return 0;
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
            if (p != null)
                p.SetValue(o, value, null);
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
            return p == null ? null : p.GetValue(o, null);
        }

        private static IEnumerable<object> Items(object list)
        {
            var e = list as System.Collections.IEnumerable;
            if (e == null) yield break;
            foreach (object o in e) yield return o;
        }

        private static string Str(object o) { return o == null ? "" : o.ToString(); }

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
            if (m != null) m.Invoke(hw, null);
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
                var item = new Sensor();
                item.Type = Str(GetProp(s, "SensorType"));
                item.Name = Str(GetProp(s, "Name"));
                item.Value = SensorValue(s);
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
            float cpuTemp = -1, cpuFreq = -1, gpuTemp = -1, gpuUsage = -1, mbdTemp = -1;
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
            foreach (char c in s ?? "")
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
}
