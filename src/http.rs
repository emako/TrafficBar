//! Minimal HTTPS GET over WinHTTP (`CCommon::GetURL`).

use windows::Win32::Networking::WinHttp::*;
use windows::core::PCWSTR;

use crate::win::wide;

pub fn get(url: &str, user_agent: &str) -> Option<String> {
    let wurl = wide(url);
    let mut host = vec![0u16; 256];
    let mut path = vec![0u16; 2048];
    let mut comps = URL_COMPONENTS {
        dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
        lpszHostName: windows::core::PWSTR(host.as_mut_ptr()),
        dwHostNameLength: host.len() as u32,
        lpszUrlPath: windows::core::PWSTR(path.as_mut_ptr()),
        dwUrlPathLength: path.len() as u32,
        ..Default::default()
    };
    unsafe {
        WinHttpCrackUrl(&wurl[..wurl.len() - 1], 0, &mut comps).ok()?;
        let ua = wide(user_agent);
        let session = WinHttpOpen(
            PCWSTR(ua.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session, 5000, 5000, 8000, 8000);
        let result = (|| {
            let connect = WinHttpConnect(session, PCWSTR(host.as_ptr()), comps.nPort, 0);
            if connect.is_null() {
                return None;
            }
            let secure = comps.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS;
            let request = WinHttpOpenRequest(
                connect,
                windows::core::w!("GET"),
                PCWSTR(path.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                if secure {
                    WINHTTP_FLAG_SECURE
                } else {
                    WINHTTP_OPEN_REQUEST_FLAGS(0)
                },
            );
            if request.is_null() {
                let _ = WinHttpCloseHandle(connect);
                return None;
            }
            let mut body = Vec::new();
            let ok = WinHttpSendRequest(request, None, None, 0, 0, 0).is_ok()
                && WinHttpReceiveResponse(request, std::ptr::null_mut()).is_ok();
            if ok {
                let mut status = 0u32;
                let mut size = std::mem::size_of::<u32>() as u32;
                let _ = WinHttpQueryHeaders(
                    request,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    PCWSTR::null(),
                    Some(&mut status as *mut u32 as *mut _),
                    &mut size,
                    std::ptr::null_mut(),
                );
                if (200..300).contains(&status) {
                    loop {
                        let mut avail = 0u32;
                        if WinHttpQueryDataAvailable(request, &mut avail).is_err() || avail == 0 {
                            break;
                        }
                        let mut buf = vec![0u8; avail as usize];
                        let mut read = 0u32;
                        if WinHttpReadData(request, buf.as_mut_ptr() as *mut _, avail, &mut read)
                            .is_err()
                            || read == 0
                        {
                            break;
                        }
                        body.extend_from_slice(&buf[..read as usize]);
                    }
                }
            }
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            if body.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(&body).into_owned())
            }
        })();
        let _ = WinHttpCloseHandle(session);
        result
    }
}
