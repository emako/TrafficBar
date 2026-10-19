/*********************************************************
* TrafficMonitor plugin interface
* Copyright (C) by Zhong Yang 2021
* zhongyang219@hotmail.com
**********************************************************/
#pragma once

class IPluginDrawer;

// Interface for one item a plugin displays.
class IPluginItem
{
public:
    /**
     * @brief   Display name of the item.
     * @return  const wchar_t*
     */
    virtual const wchar_t* GetItemName() const = 0;

    /**
     * @brief   Unique id of the item.
     * @return  const wchar_t*
     */
    virtual const wchar_t* GetItemId() const = 0;

    /**
     * @brief   Label text of the item.
     * @return  const wchar_t*
     */
    virtual const wchar_t* GetItemLableText() const = 0;

    /**
     * @brief   Value text of the item.
     * @detail  This can be called often. Do not sample monitor data here.
     *  Sample in ITMPlugin::DataRequired, store the result, and return it from here.
     * @return  const wchar_t*
     */
    virtual const wchar_t* GetItemValueText() const = 0;

    /**
     * @brief   Sample value text.
     * @detail  The length of the returned string is used to measure the display width.
     * @return  const wchar_t*
     */
    virtual const wchar_t* GetItemValueSampleText() const = 0;

    /**
     * @brief   Whether the plugin draws the display area itself.
     * @detail
     *  Returning false lets the host draw GetItemLableText and GetItemValueText. DrawItem is ignored.
     *  Returning true requires an override of DrawItem that paints the area.
     *  GetItemLableText, GetItemValueText, and GetItemValueSampleText are then ignored.
     * @return  bool
     */
    virtual bool IsCustomDraw() const { return false; }

    /**
     * @brief   Width of the display area.
     * @detail
     *  Only used when CustomDraw() returns true.
     *  The value is the width at 96 DPI (100%). The host scales it for the current DPI,
     *  so do not return a different value per DPI.
     *  This is the minimum width the custom area needs. The w argument of DrawItem may be larger.
     * @return  int
     */
    virtual int GetItemWidth() const { return 0; }

    /**
     * @brief   Paints the display area. Only used when CustomDraw() returns true.
     * @param   void * hDC Device context to draw on.
     * @param   int x Rectangle to draw in.
     * @param   int y
     * @param   int w
     * @param   int h
     * @param   bool dark_mode true for dark mode, false for light mode.
     * @return  void
     */
    virtual void DrawItem(void* hDC, int x, int y, int w, int h, bool dark_mode) {}

    /**
     * @brief   Width of the display area.
     * @detail
     *  Only used when CustomDraw() returns true.
     *  Unlike GetItemWidth, the plugin may measure against hDC.
     *  The return value is the real width. The host does not scale it for DPI.
     *  This is the minimum width the custom area needs. The w argument of DrawItem may be larger.
     * @param   void * hDC Device context to measure with.
     * @return  int
     */
    virtual int GetItemWidthEx(void* hDC) const { return 0; }

    /** Kind of mouse event. */
    enum MouseEventType
    {
        MT_LCLICKED,    /**< Left button clicked. */
        MT_RCLICKED,    /**< Right button clicked. */
        MT_DBCLICKED,   /**< Left button double-clicked. */
        MT_WHEEL_UP,    /**< Wheel scrolled up. */
        MT_WHEEL_DOWN,  /**< Wheel scrolled down. */
    };

    enum MouseEventFlag
    {
        MF_TASKBAR_WND = 1 << 0,        /**< The event came from the taskbar window. */
    };

    /**
     * @brief   Called by the host when the pointer is over the plugin area.
     * @param   MouseEventType type Kind of mouse event.
     * @param   int x Pointer x, in the window.
     * @param   int y Pointer y, in the window.
     * @param   void* hWnd Window that received the event (main window or taskbar window).
     * @param   int flag Combination of MouseEventFlag values.
     * @return  int Non-zero means the plugin fully handled the event and the host must not handle it again.
     *   For MT_RCLICKED, 0 lets the host open its context menu. Non-zero means the host does nothing.
     */
    virtual int OnMouseEvent(MouseEventType type, int x, int y, void* hWnd, int flag) { return 0; }

    enum KeyboardEventFlag
    {
        KF_TASKBAR_WND = 1 << 0,        /**< The event came from the taskbar window. */
    };

    /**
     * @brief   Called by the host when the plugin area receives a key.
     * @param   int key Key that was pressed.
     * @param   bool ctrl Ctrl is down.
     * @param   bool shift Shift is down.
     * @param   bool alt Alt is down.
     * @param   void* hWnd Window that received the event (main window or taskbar window).
     * @param   int flag Combination of KeyboardEventFlag values.
     * @return  int Non-zero means the plugin fully handled the event and the host must not handle it again.
     */
    virtual int OnKeboardEvent(int key, bool ctrl, bool shift, bool alt, void* hWnd, int flag) { return 0; }

    enum ItemInfoType
    {

    };
    // Reserved.
    virtual void* OnItemInfo(ItemInfoType, void* para1, void* para2) { return 0; }

    /**
     * @brief   Whether to draw this item's resource-usage graph on the taskbar.
     * @return  1 to draw, 0 to skip.
     */
    virtual int IsDrawResourceUsageGraph() const { return 0; }

    /**
     * @brief   Value of the resource-usage graph. Used when IsDrawResourceUsageGraph() is non-zero.
     * @return  float Graph value, from 0.0 to 1.0.
     */
    virtual float GetResourceUsageGraphValue() const { return 0.0; }

    /**
     * @brief   Paints the display area. Only used when CustomDraw() returns true.
     * @param   IPluginDrawer* pDrawer Drawing interface for the plugin.
     * @param   int x Rectangle to draw in.
     * @param   int y
     * @param   int w
     * @param   int h
     * @param   bool dark_mode true for dark mode, false for light mode.
     * @return  bool
     */
    virtual bool DrawItemEx(IPluginDrawer* pDrawer, int x, int y, int w, int h, bool dark_mode) { return false; }

    /**
     * @brief   Whether this item takes both lines by itself on the taskbar.
     * @return  1 for yes, 0 for no.
     */
    virtual int IsDoubleLineExclusive() const { return 0; }
};

class ITrafficMonitor;

///////////////////////////////////////////////////////////////////////////////////////////////////////
// Plugin interface.
class ITMPlugin
{
public:
    /**
     * @brief   Plugin API version. Changes only when this interface changes.
     * @attention Plugin authors must not change this return value or override this function.
     * @return  int
     */
    virtual int GetAPIVersion() const { return 8; }

    /**
     * @brief   Returns one display item.
     * @detail  One plugin DLL may expose several IPluginItem objects, one per display item.
     *  Return the item when index is at least 0 and less than the item count. Otherwise return null.
     *  A plugin with two items returns them for index 0 and 1, and null for every other index.
     * @param   int index Index of the object.
     * @return  IPluginItem* The display item.
     */
    virtual IPluginItem* GetItem(int index) = 0;

    /**
     * @brief   Called on a timer. The plugin should sample its monitor data here.
     */
    virtual void DataRequired() = 0;

    /** Result of the plugin options dialog. */
    enum OptionReturn
    {
        OR_OPTION_CHANGED,          /**< The dialog changed settings. */
        OR_OPTION_UNCHANGED,        /**< The dialog did not change settings. */
        OR_OPTION_NOT_PROVIDED      /**< The plugin has no options dialog. */
    };

    /**
     * @brief   Called by the host to open the plugin options dialog.
     * @detail  Optional. A plugin with an options UI should override this and return OR_OPTION_CHANGED or OR_OPTION_UNCHANGED.
     * @param   void * hParent Parent window handle.
     *  OR_OPTION_NOT_PROVIDED means the plugin has no options dialog.
     * @return  ITMPlugin::OptionReturn
     */
    virtual OptionReturn ShowOptionsDialog(void* hParent) { return OR_OPTION_NOT_PROVIDED; }

    /** Index of a plugin information string. */
    enum PluginInfoIndex
    {
        TMI_NAME,           /**< Name. */
        TMI_DESCRIPTION,    /**< Description. */
        TMI_AUTHOR,         /**< Author. */
        TMI_COPYRIGHT,      /**< Copyright. */
        TMI_VERSION,        /**< Version. */
        TMI_URL,            /**< Home page. */
        TMI_MAX             /**< One past the last info index. */
    };

    /**
     * @brief   Plugin information for the given index.
     */
    virtual const wchar_t* GetInfo(PluginInfoIndex index) = 0;

    /** Monitor values collected by the host. */
    struct MonitorInfo
    {
        unsigned long long up_speed{};
        unsigned long long down_speed{};
        int cpu_usage{};
        int memory_usage{};
        int gpu_usage{};
        int hdd_usage{};
        int cpu_temperature{};
        int gpu_temperature{};
        int hdd_temperature{};
        int main_board_temperature{};
        int cpu_freq{};
    };

    /**
     * @brief   Called by the host with every monitor value it has collected.
     */
    virtual void OnMonitorInfo(const MonitorInfo& monitor_info) {}

    /**
     * @brief   Text the plugin wants shown in the mouse tooltip.
     */
    virtual const wchar_t* GetTooltipInfo() { return L""; }

    enum ExtendedInfoIndex
    {
        EI_LABEL_TEXT_COLOR,    // Label color used for drawing.
        EI_VALUE_TEXT_COLOR,    // Value color used for drawing.
        EI_DRAW_TASKBAR_WND,    // Whether the taskbar window is being drawn.

        // Main-window options.
        EI_NAIN_WND_NET_SPEED_SHORT_MODE,   // Short network-speed display.
        EI_MAIN_WND_SPERATE_WITH_SPACE,     // Separate the value and the unit with a space.
        EI_MAIN_WND_UNIT_BYTE,              // Whether the speed unit is B (bytes).
        EI_MAIN_WND_UNIT_SELECT,            // Speed unit: 0 auto, 1 fixed KB/s, 2 fixed MB/s.
        EI_MAIN_WND_NOT_SHOW_UNIT,          // Hide the speed unit.
        EI_MAIN_WND_NOT_SHOW_PERCENT,       // Hide the percent sign.

        // Taskbar-window options.
        EI_TASKBAR_WND_NET_SPEED_SHORT_MODE,    // Short network-speed display.
        EI_TASKBAR_WND_SPERATE_WITH_SPACE,      // Separate the value and the unit with a space.
        EI_TASKBAR_WND_VALUE_RIGHT_ALIGN,       // Right-align values.
        EI_TASKBAR_WND_NET_SPEED_WIDTH,         // Width of the speed text.
        EI_TASKBAR_WND_UNIT_BYTE,               // Whether the speed unit is B (bytes).
        EI_TASKBAR_WND_UNIT_SELECT,             // Speed unit: 0 auto, 1 fixed KB/s, 2 fixed MB/s.
        EI_TASKBAR_WND_NOT_SHOW_UNIT,           // Hide the speed unit.
        EI_TASKBAR_WND_NOT_SHOW_PERCENT,        // Hide the percent sign.

        EI_CONFIG_DIR,                      // Directory of the config files.
    };

    /**
     * @brief   Called by the host to pass extra information to the plugin.
     * @param   ExtendedInfoIndex index Which piece of information this is.
     * @param   const wchar_t* data The value.
     * @return  void
     */
    virtual void OnExtenedInfo(ExtendedInfoIndex index, const wchar_t* data) {}

    /**
     * @brief   Plugin icon, as an HICON.
     */
    virtual void* GetPluginIcon() { return nullptr; }

    /**
     * @brief   Number of plugin commands.
     * @return  int Number of plugin commands.
     */
    virtual int GetCommandCount() { return 0; }

    /**
     * @brief   Name of a plugin command.
     * @param   int command_index Command index, from 0 and less than GetCommandCount().
     * @return  wchar_t* Command name.
     */
    virtual const wchar_t* GetCommandName(int command_index) { return nullptr; }

    /**
     * @brief   Icon of a plugin command.
     * @param   int command_index Command index, from 0 and less than GetCommandCount().
     * @return  void* Command icon, as an HICON.
     */
    virtual void* GetCommandIcon(int command_index) { return nullptr; }

    /**
     * @brief   Called by the host to run a plugin command.
     * @param   int command_index Command index, from 0 and less than GetCommandCount().
     * @param   void* hWnd Window that sent the command.
     * @param   void* para Reserved.
     */
    virtual void OnPluginCommand(int command_index, void* hWnd, void* para) {}

    /**
    * @brief   Whether a plugin command is checked.
    * @param   int command_index Command index, from 0 and less than GetCommandCount().
    * @return  1 checked, 0 not checked.
    */
    virtual int IsCommandChecked(int command_index) { return false; }

    /**
     * @brief   Plugin startup.
     * @detail  Called when the plugin is loaded, with the ITrafficMonitor pointer.
     *  The plugin may keep the pointer and call through it.
     * @param   pApp
     */
    virtual void OnInitialize(ITrafficMonitor* pApp) {}
};


///////////////////////////////////////////////////////////////////////////////////////////////////////
// Host interface.
class ITrafficMonitor
{
public:
    /**
     * @brief   Version of this interface.
     * @attention Check the version before calling other ITrafficMonitor functions.
     * @return  int
     */
    virtual int GetAPIVersion() = 0;

    /**
     * @brief   TrafficMonitor version string.
     * @return  const wchar_t*
     */
    virtual const wchar_t* GetVersion() = 0;

    /** Monitor values exposed by the host. */
    enum MonitorItem
    {
        MI_UP,                  /**< Upload speed. */
        MI_DOWN,                /**< Download speed. */
        MI_CPU,                 /**< CPU usage. */
        MI_MEMORY,              /**< Memory usage. */
        MI_GPU_USAGE,           /**< GPU usage. */
        MI_CPU_TEMP,            /**< CPU temperature. */
        MI_GPU_TEMP,            /**< GPU temperature. */
        MI_HDD_TEMP,            /**< Hard-disk temperature. */
        MI_MAIN_BOARD_TEMP,     /**< Mainboard temperature. */
        MI_HDD_USAGE,           /**< Hard-disk usage. */
        MI_CPU_FREQ,            /**< CPU frequency. */
        MI_TODAY_UP_TRAFFIC,    /**< Upload traffic today. */
        MI_TODAY_DOWN_TRAFFIC   /**< Download traffic today. */
    };

    /**
     * @brief   Numeric value of one host monitor item.
     *          (ITMPlugin::OnMonitorInfo will be removed.)
     * @param   item Which value to read.
     * @return  The value.
     */
    virtual double GetMonitorValue(MonitorItem item) = 0;

    /**
     * @brief   String form of one host monitor item.
     * @param   item Which value to read.
     * @param   is_main_window Non-zero for the main-window formatting.
     * @return  The formatted value.
     */
    virtual const wchar_t* GetMonitorValueString(MonitorItem item, int is_main_window = false) = 0;

    /**
     * @brief   Shows a notification.
     * @param   strMsg Notification text.
     */
    virtual void ShowNotifyMessage(const wchar_t* strMsg) = 0;

    /**
     * @brief   Current language id.
     * @return  Language id of the host.
     */
    virtual unsigned short GetLanguageId() const = 0;

    /**
     * @brief   Config directory.
     * @return  Config directory.
     */
    virtual const wchar_t* GetPluginConfigDir() const = 0;

    /** Which window's DPI to read. */
    enum DPIType
    {
        DPI_MAIN_WND,   /**< DPI of the main window. */
        DPI_TASKBAR     /**< DPI of the taskbar window. */
    };

    /**
     * @brief   Host DPI.
     * @return  Host DPI.
     */
    virtual int GetDPI(DPIType type) const = 0;

    /**
     * @brief   Current system theme color.
     * @return  Color as COLORREF.
     */
    virtual unsigned int GetThemeColor() const = 0;

    /**
     * @brief   String from the current language table.
     * @param   str_id String id.
     * @param   str_section Ini section name.
     * @return
     */
    virtual const wchar_t* GetStringRes(const wchar_t* key, const wchar_t* section) = 0;

    /**
     * @brief   Handle of the main window.
     * @return
     */
    virtual void* GetMainWindowHwnd() = 0;

    /**
     * @brief   Handle of the taskbar window. Null when that window is hidden.
     * @return
     */
    virtual void* GetTaskbarWindowHwnd() = 0;

    /**
     * API Version
     *      1       Added GetMainWindowHwnd and GetTaskbarWindowHwnd.
     */
};


// Plugin drawing interface.
class IPluginDrawer
{
public:
    /**
     * @brief   Version of this interface.
     * @return  int
     */
    virtual int GetAPIVersion() = 0;

    // How a bitmap is fitted into the rectangle.
    enum StretchMode
    {
        STRETCH,    /**< Stretch. The aspect ratio may change. */
        FILL,       /**< Fill. The aspect ratio is kept and the long side is cropped. */
        FIT         /**< Fit. The aspect ratio is kept and nothing is cropped. */
    };

    // Text alignment.
    enum Alignment
    {
        LEFT,       /**< Align left. */
        RIGHT,      /**< Align right. */
        CENTER,     /**< Center. */
    };

    /**
     * @brief   Draws text in a rectangle.
     * @param   x Left of the rectangle.
     * @param   y Top of the rectangle.
     * @param   w Width of the rectangle.
     * @param   h Height of the rectangle.
     * @param   lpszString Text to draw.
     * @param   color Text color, 0xBBGGRR.
     * @param   align Text alignment.
     * @param   multi_line Whether to wrap onto multiple lines.
     * @param   alpha Opacity, 0 to 255.
     */
    virtual void DrawWindowText(int x, int y, int w, int h, const wchar_t* lpszString, unsigned long color, Alignment align = Alignment::LEFT, bool multi_line = false, unsigned char alpha = 255) = 0;

    /**
     * @brief   Sets the clipping rectangle.
     * @param   x Left of the rectangle.
     * @param   y Top of the rectangle.
     * @param   w Width of the rectangle.
     * @param   h Height of the rectangle.
     */
    virtual void SetDrawRect(int x, int y, int w, int h) = 0;

    /**
     * @brief   Fills a rectangle with a solid color.
     * @param   x Left of the rectangle.
     * @param   y Top of the rectangle.
     * @param   w Width of the rectangle.
     * @param   h Height of the rectangle.
     * @param   color Fill color, 0xBBGGRR.
     * @param   alpha Opacity, 0 to 255.
     */
    virtual void FillRect(int x, int y, int w, int h, unsigned long color, unsigned char alpha = 255) = 0;

    /**
     * @brief   Draws a rectangle outline.
     * @param   x Left of the rectangle.
     * @param   y Top of the rectangle.
     * @param   w Width of the rectangle.
     * @param   h Height of the rectangle.
     * @param   color Outline color, 0xBBGGRR.
     * @param   width Line width.
     * @param   dot_line Whether the line is dashed.
     * @param   alpha Opacity, 0 to 255.
     * @param   radius Corner radius. 0 is a sharp rectangle.
     */
    virtual void DrawRectOutLine(int x, int y, int w, int h, unsigned long color, int width = 1, bool dot_line = false, unsigned char alpha = 255, int radius = 0) = 0;

    /**
     * @brief   Draws a bitmap.
     * @param   hbitmap Bitmap handle.
     * @param   x Left of the rectangle.
     * @param   y Top of the rectangle.
     * @param   w Width of the rectangle.
     * @param   h Height of the rectangle.
     * @param   stretch_mode How the bitmap is fitted.
     * @param   alpha Opacity, 0 to 255.
     */
    virtual void DrawBitmap(void* hbitmap, int x, int y, int w, int h, StretchMode stretch_mode = StretchMode::STRETCH, unsigned char alpha = 255) = 0;

    /**
     * @brief   Draws an icon.
     * @param   hIcon Icon handle.
     * @param   x Left of the rectangle.
     * @param   y Top of the rectangle.
     * @param   w Width of the rectangle.
     * @param   h Height of the rectangle.
     */
    virtual void DrawIcon(void* hIcon, int x, int y, int w, int h) = 0;

    /**
     * @brief   Measures the width and height of a string.
     * @param   lpszString The string.
     * @param[out]  w Width of the string.
     * @param[out]  h Height of the string.
     */
    virtual void GetTextExtent(const wchar_t* lpszString, int& w, int& h) = 0;

    /**
     * @brief   Draws a line.
     * @param   x1 X of the first point.
     * @param   y1 Y of the first point.
     * @param   x2 X of the second point.
     * @param   y2 Y of the second point.
     * @param   color Color.
     * @param   alpha Opacity.
     */
    virtual void DrawLine(int x1, int y1, int x2, int y2, unsigned long color, int width = 1, bool dot_line = false, unsigned char alpha = 255) = 0;

    /**
     * @brief   GDI device context used for drawing.
     * @return  Handle as an HDC.
     */
    virtual void* GetHDC() = 0;
};


/*
* A plugin DLL must export:
* ITMPlugin* TMPluginGetInstance();
*   The function returns an ITMPlugin.
*   That object should be global or static and must stay alive until the process exits.
*/


/*
* Changelog:
* ITMPlugin
* -------------------------------------------------------------------------
* API version |                       Change
* -------------------------------------------------------------------------
*     1       | First version
* -------------------------------------------------------------------------
*     2       | Added ITMPlugin::GetTooltipInfo
* -------------------------------------------------------------------------
*     3       | Added IPluginItem::GetItemWidthEx and IPluginItem::OnMouseEvent
* -------------------------------------------------------------------------
*     4       | Added IPluginItem::OnKeboardEvent and IPluginItem::OnItemInfo
* -------------------------------------------------------------------------
*     5       | Added ITMPlugin::GetCommandName, ITMPlugin::GetCommandIcon, and ITMPlugin::OnPluginCommand
* -------------------------------------------------------------------------
*     6       | Added IPluginItem::GetResourceUsageGraphType and IPluginItem::GetResourceUsageGraphValue
* -------------------------------------------------------------------------
*     7       | Added ITMPlugin::OnInitialize
* -------------------------------------------------------------------------
*     8       | Added IPluginItem::DrawItemEx and IPluginItem::IsDoubleLineExclusive
* -------------------------------------------------------------------------
*/
