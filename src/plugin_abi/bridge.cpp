// C ABI over the TrafficMonitor plugin interface (MSVC C++ vtables). Rust only talks to the
// functions declared in the extern "C" block; every call into a plugin checks the plugin's API
// version first, because older plugins do not have the newer virtual functions in their vtable.
#include <windows.h>
#include "PluginInterface.h"

extern "C" {

struct TmHostCallbacks
{
    double (*get_monitor_value)(int item);
    const wchar_t* (*get_monitor_value_string)(int item, int is_main_window);
    void (*show_notify_message)(const wchar_t* msg);
    unsigned short (*get_language_id)();
    const wchar_t* (*get_config_dir)();
    int (*get_dpi)(int type);
    unsigned int (*get_theme_color)();
    const wchar_t* (*get_string_res)(const wchar_t* key, const wchar_t* section);
    void* (*get_taskbar_hwnd)();
};

struct TmMonitorInfo
{
    unsigned long long up_speed;
    unsigned long long down_speed;
    int cpu_usage;
    int memory_usage;
    int gpu_usage;
    int hdd_usage;
    int cpu_temperature;
    int gpu_temperature;
    int hdd_temperature;
    int main_board_temperature;
    int cpu_freq;
};

}

namespace
{
    TmHostCallbacks g_host{};
    const wchar_t* Safe(const wchar_t* s) { return s ? s : L""; }

    class Host : public ITrafficMonitor
    {
    public:
        int GetAPIVersion() override { return 1; }
        const wchar_t* GetVersion() override { return L"1.86"; }
        double GetMonitorValue(MonitorItem item) override
        {
            return g_host.get_monitor_value ? g_host.get_monitor_value(static_cast<int>(item)) : 0.0;
        }
        const wchar_t* GetMonitorValueString(MonitorItem item, int is_main_window) override
        {
            return g_host.get_monitor_value_string ? Safe(g_host.get_monitor_value_string(static_cast<int>(item), is_main_window)) : L"";
        }
        void ShowNotifyMessage(const wchar_t* strMsg) override
        {
            if (g_host.show_notify_message)
                g_host.show_notify_message(Safe(strMsg));
        }
        unsigned short GetLanguageId() const override
        {
            return g_host.get_language_id ? g_host.get_language_id() : 0;
        }
        const wchar_t* GetPluginConfigDir() const override
        {
            return g_host.get_config_dir ? Safe(g_host.get_config_dir()) : L"";
        }
        int GetDPI(DPIType type) const override
        {
            return g_host.get_dpi ? g_host.get_dpi(static_cast<int>(type)) : 96;
        }
        unsigned int GetThemeColor() const override
        {
            return g_host.get_theme_color ? g_host.get_theme_color() : 0;
        }
        const wchar_t* GetStringRes(const wchar_t* key, const wchar_t* section) override
        {
            return g_host.get_string_res ? Safe(g_host.get_string_res(Safe(key), Safe(section))) : L"";
        }
        void* GetMainWindowHwnd() override { return nullptr; }
        void* GetTaskbarWindowHwnd() override
        {
            return g_host.get_taskbar_hwnd ? g_host.get_taskbar_hwnd() : nullptr;
        }
    };

    Host g_host_object;

    typedef ITMPlugin* (*pfTMPluginGetInstance)();

    inline ITMPlugin* P(void* p) { return static_cast<ITMPlugin*>(p); }
    inline IPluginItem* I(void* p) { return static_cast<IPluginItem*>(p); }
}

extern "C" {

void tm_set_host(const TmHostCallbacks* callbacks)
{
    if (callbacks)
        g_host = *callbacks;
}

void* tm_host_object() { return static_cast<ITrafficMonitor*>(&g_host_object); }

// Returns the ITMPlugin* exported by the module, or null.
void* tm_get_instance(HMODULE module)
{
    if (!module)
        return nullptr;
    auto fn = reinterpret_cast<pfTMPluginGetInstance>(::GetProcAddress(module, "TMPluginGetInstance"));
    if (!fn)
        return nullptr;
    __try
    {
        return fn();
    }
    __except (EXCEPTION_EXECUTE_HANDLER)
    {
        return nullptr;
    }
}

int tm_plugin_api_version(void* plugin) { return P(plugin)->GetAPIVersion(); }

void* tm_plugin_get_item(void* plugin, int index) { return P(plugin)->GetItem(index); }

void tm_plugin_data_required(void* plugin) { P(plugin)->DataRequired(); }

int tm_plugin_show_options(void* plugin, void* parent)
{
    return static_cast<int>(P(plugin)->ShowOptionsDialog(parent));
}

const wchar_t* tm_plugin_get_info(void* plugin, int index)
{
    return Safe(P(plugin)->GetInfo(static_cast<ITMPlugin::PluginInfoIndex>(index)));
}

void tm_plugin_on_monitor_info(void* plugin, const TmMonitorInfo* info)
{
    ITMPlugin::MonitorInfo mi;
    mi.up_speed = info->up_speed;
    mi.down_speed = info->down_speed;
    mi.cpu_usage = info->cpu_usage;
    mi.memory_usage = info->memory_usage;
    mi.gpu_usage = info->gpu_usage;
    mi.hdd_usage = info->hdd_usage;
    mi.cpu_temperature = info->cpu_temperature;
    mi.gpu_temperature = info->gpu_temperature;
    mi.hdd_temperature = info->hdd_temperature;
    mi.main_board_temperature = info->main_board_temperature;
    mi.cpu_freq = info->cpu_freq;
    P(plugin)->OnMonitorInfo(mi);
}

const wchar_t* tm_plugin_tooltip(void* plugin)
{
    if (P(plugin)->GetAPIVersion() < 2)
        return L"";
    return Safe(P(plugin)->GetTooltipInfo());
}

void tm_plugin_on_extended_info(void* plugin, int index, const wchar_t* data)
{
    if (P(plugin)->GetAPIVersion() >= 2)
        P(plugin)->OnExtenedInfo(static_cast<ITMPlugin::ExtendedInfoIndex>(index), Safe(data));
}

void* tm_plugin_icon(void* plugin)
{
    if (P(plugin)->GetAPIVersion() < 5)
        return nullptr;
    return P(plugin)->GetPluginIcon();
}

int tm_plugin_command_count(void* plugin)
{
    if (P(plugin)->GetAPIVersion() < 5)
        return 0;
    return P(plugin)->GetCommandCount();
}

const wchar_t* tm_plugin_command_name(void* plugin, int index)
{
    if (P(plugin)->GetAPIVersion() < 5)
        return L"";
    return Safe(P(plugin)->GetCommandName(index));
}

void* tm_plugin_command_icon(void* plugin, int index)
{
    if (P(plugin)->GetAPIVersion() < 5)
        return nullptr;
    return P(plugin)->GetCommandIcon(index);
}

void tm_plugin_on_command(void* plugin, int index, void* hwnd)
{
    if (P(plugin)->GetAPIVersion() >= 5)
        P(plugin)->OnPluginCommand(index, hwnd, nullptr);
}

int tm_plugin_command_checked(void* plugin, int index)
{
    if (P(plugin)->GetAPIVersion() < 5)
        return 0;
    return P(plugin)->IsCommandChecked(index);
}

void tm_plugin_on_initialize(void* plugin)
{
    if (P(plugin)->GetAPIVersion() >= 7)
        P(plugin)->OnInitialize(&g_host_object);
}

const wchar_t* tm_item_name(void* item) { return Safe(I(item)->GetItemName()); }
const wchar_t* tm_item_id(void* item) { return Safe(I(item)->GetItemId()); }
const wchar_t* tm_item_label(void* item) { return Safe(I(item)->GetItemLableText()); }
const wchar_t* tm_item_value(void* item) { return Safe(I(item)->GetItemValueText()); }
const wchar_t* tm_item_sample(void* item) { return Safe(I(item)->GetItemValueSampleText()); }
int tm_item_is_custom_draw(void* item) { return I(item)->IsCustomDraw() ? 1 : 0; }
int tm_item_width(void* item) { return I(item)->GetItemWidth(); }

int tm_item_on_mouse_event(void* plugin, void* item, int type, int x, int y, void* hwnd, int flag)
{
    if (P(plugin)->GetAPIVersion() < 3)
        return 0;
    return I(item)->OnMouseEvent(static_cast<IPluginItem::MouseEventType>(type), x, y, hwnd, flag);
}

int tm_item_on_keyboard_event(void* plugin, void* item, int key, int ctrl, int shift, int alt, void* hwnd, int flag)
{
    if (P(plugin)->GetAPIVersion() < 4)
        return 0;
    return I(item)->OnKeboardEvent(key, ctrl != 0, shift != 0, alt != 0, hwnd, flag);
}

int tm_item_is_draw_graph(void* plugin, void* item)
{
    if (P(plugin)->GetAPIVersion() < 6)
        return 0;
    return I(item)->IsDrawResourceUsageGraph();
}

float tm_item_graph_value(void* plugin, void* item)
{
    if (P(plugin)->GetAPIVersion() < 6)
        return 0.0f;
    return I(item)->GetResourceUsageGraphValue();
}

int tm_item_double_line(void* plugin, void* item)
{
    if (P(plugin)->GetAPIVersion() < 8)
        return 0;
    return I(item)->IsDoubleLineExclusive();
}

}
