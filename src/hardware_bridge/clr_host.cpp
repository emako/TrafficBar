// In-process CLR host. Rust passes the embedded assembly bytes and the Dispatch
// argument string. The assembly is loaded once with _AppDomain::Load_3 and is never
// written to disk. COM pointers are kept until process exit; ICorRuntimeHost::Stop
// can hang after managed code has run.
#include <windows.h>
#include <metahost.h>
#include <mscoree.h>
#include <oleauto.h>

#include <cstdint>
#include <cstring>

#import "mscorlib.tlb" raw_interfaces_only \
    high_property_prefixes("_get", "_put", "_putref") \
    rename("ReportEvent", "InteropServices_ReportEvent") \
    rename("or", "InteropServices_or")

namespace {

constexpr int kInvokeStatic = 8 | 16 | 256; // BindingFlags.Static | Public | InvokeMethod

struct Bstr {
    BSTR value = nullptr;
    explicit Bstr(const wchar_t* text) : value(SysAllocString(text ? text : L"")) {}
    ~Bstr() { SysFreeString(value); }
    Bstr(const Bstr&) = delete;
    Bstr& operator=(const Bstr&) = delete;
};

struct Variant {
    VARIANT value{};
    Variant() { VariantInit(&value); }
    ~Variant() { VariantClear(&value); }
    Variant(const Variant&) = delete;
    Variant& operator=(const Variant&) = delete;
};

struct SafeArray {
    SAFEARRAY* value = nullptr;
    SafeArray() = default;
    ~SafeArray() {
        if (value)
            SafeArrayDestroy(value);
    }
    SafeArray(const SafeArray&) = delete;
    SafeArray& operator=(const SafeArray&) = delete;
};

ICorRuntimeHost* g_host = nullptr;
mscorlib::_AppDomain* g_domain = nullptr;
mscorlib::_Assembly* g_assembly = nullptr;
mscorlib::_Type* g_bridge = nullptr;

using ClrCreateInstanceFn = HRESULT(STDAPICALLTYPE*)(REFCLSID clsid, REFIID riid, LPVOID* ppInterface);

HRESULT EnsureDomain() {
    if (g_domain)
        return S_OK;

    const HRESULT init = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (FAILED(init) && init != RPC_E_CHANGED_MODE)
        return init;

    if (!g_host) {
        const HMODULE module = LoadLibraryW(L"mscoree.dll");
        if (!module)
            return HRESULT_FROM_WIN32(GetLastError());
        const auto create = reinterpret_cast<ClrCreateInstanceFn>(GetProcAddress(module, "CLRCreateInstance"));
        if (!create)
            return HRESULT_FROM_WIN32(ERROR_PROC_NOT_FOUND);

        ICLRMetaHost* meta = nullptr;
        HRESULT hr = create(CLSID_CLRMetaHost, IID_ICLRMetaHost, reinterpret_cast<LPVOID*>(&meta));
        if (FAILED(hr) || !meta)
            return FAILED(hr) ? hr : E_FAIL;

        ICLRRuntimeInfo* info = nullptr;
        hr = meta->GetRuntime(L"v4.0.30319", IID_ICLRRuntimeInfo, reinterpret_cast<LPVOID*>(&info));
        meta->Release();
        if (FAILED(hr) || !info)
            return FAILED(hr) ? hr : E_FAIL;

        ICorRuntimeHost* host = nullptr;
        hr = info->GetInterface(CLSID_CorRuntimeHost, IID_ICorRuntimeHost, reinterpret_cast<LPVOID*>(&host));
        info->Release();
        if (FAILED(hr) || !host)
            return FAILED(hr) ? hr : E_FAIL;
        hr = host->Start();
        if (FAILED(hr)) {
            host->Release();
            return hr;
        }
        g_host = host;
    }

    IUnknown* unknown = nullptr;
    const HRESULT hrDomain = g_host->GetDefaultDomain(&unknown);
    if (FAILED(hrDomain) || !unknown)
        return FAILED(hrDomain) ? hrDomain : E_FAIL;
    mscorlib::_AppDomain* domain = nullptr;
    const HRESULT hrQuery = unknown->QueryInterface(__uuidof(mscorlib::_AppDomain), reinterpret_cast<void**>(&domain));
    unknown->Release();
    if (FAILED(hrQuery) || !domain)
        return FAILED(hrQuery) ? hrQuery : E_NOINTERFACE;
    g_domain = domain;
    return S_OK;
}

HRESULT EnsureAssembly(const uint8_t* bytes, size_t len) {
    if (g_assembly)
        return S_OK;
    if (!bytes || len == 0 || len > 0xFFFFFFFFu)
        return E_INVALIDARG;

    const HRESULT hrDomain = EnsureDomain();
    if (FAILED(hrDomain))
        return hrDomain;

    SafeArray image;
    SAFEARRAYBOUND bound{};
    bound.lLbound = 0;
    bound.cElements = static_cast<ULONG>(len);
    image.value = SafeArrayCreate(VT_UI1, 1, &bound);
    if (!image.value)
        return E_OUTOFMEMORY;
    void* data = nullptr;
    const HRESULT hrAccess = SafeArrayAccessData(image.value, &data);
    if (FAILED(hrAccess))
        return hrAccess;
    std::memcpy(data, bytes, len);
    SafeArrayUnaccessData(image.value);

    mscorlib::_Assembly* assembly = nullptr;
    const HRESULT hrLoad = g_domain->Load_3(image.value, &assembly);
    if (FAILED(hrLoad) || !assembly)
        return FAILED(hrLoad) ? hrLoad : E_FAIL;
    g_assembly = assembly;
    return S_OK;
}

HRESULT EnsureBridge(const uint8_t* bytes, size_t len) {
    if (g_bridge)
        return S_OK;
    const HRESULT hrAssembly = EnsureAssembly(bytes, len);
    if (FAILED(hrAssembly))
        return hrAssembly;

    const Bstr name(L"TrafficBar.Hardware.Bridge");
    if (!name.value)
        return E_OUTOFMEMORY;
    mscorlib::_Type* bridge = nullptr;
    const HRESULT hrType = g_assembly->GetType_2(name.value, &bridge);
    if (FAILED(hrType) || !bridge)
        return FAILED(hrType) ? hrType : E_FAIL;
    g_bridge = bridge;
    return S_OK;
}

HRESULT InvokeDispatch(const wchar_t* arg, uint32_t* ret) {
    const Bstr method(L"Dispatch");
    if (!method.value)
        return E_OUTOFMEMORY;

    SafeArray args;
    args.value = SafeArrayCreateVector(VT_VARIANT, 0, 1);
    if (!args.value)
        return E_OUTOFMEMORY;

    Variant item;
    item.value.vt = VT_BSTR;
    item.value.bstrVal = SysAllocString(arg ? arg : L"");
    if (!item.value.bstrVal)
        return E_OUTOFMEMORY;
    LONG index = 0;
    const HRESULT hrPut = SafeArrayPutElement(args.value, &index, &item.value);
    if (FAILED(hrPut))
        return hrPut;

    Variant target;
    Variant result;
    const HRESULT hrInvoke = g_bridge->InvokeMember_3(
        method.value,
        static_cast<mscorlib::BindingFlags>(kInvokeStatic),
        nullptr,
        target.value,
        args.value,
        &result.value);
    if (FAILED(hrInvoke))
        return hrInvoke;
    if (result.value.vt == VT_I4 || result.value.vt == VT_INT)
        *ret = static_cast<uint32_t>(result.value.lVal);
    else if (result.value.vt == VT_UI4)
        *ret = result.value.ulVal;
    else
        return E_FAIL;
    return S_OK;
}

} // namespace

extern "C" int32_t hardware_clr_call(const uint8_t* assembly, size_t assembly_len, const wchar_t* arg, uint32_t* ret) {
    if (!ret)
        return E_POINTER;
    *ret = 0;
    const HRESULT hr = EnsureBridge(assembly, assembly_len);
    if (FAILED(hr))
        return hr;
    return InvokeDispatch(arg, ret);
}
