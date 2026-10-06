//! WMI through COM: the same classes the PowerShell cmdlets read
//! (`Get-CimInstance Win32_ComputerSystem` and the rest). Every property
//! comes back as a JSON value, strings and numbers as WMI typed them.

use serde_json::Value;
use windows::core::BSTR;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE};
use windows::Win32::System::Ole::{SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{VariantClear, VARENUM, VARIANT, VT_ARRAY, VT_BOOL, VT_BSTR, VT_EMPTY, VT_I2, VT_I4, VT_I8, VT_NULL, VT_R4, VT_R8, VT_UI1, VT_UI2, VT_UI4, VT_UI8};
use windows::Win32::System::Wmi::{IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE};

pub struct Wmi {
    services: IWbemServices,
}

impl Wmi {
    /// Connect to `ROOT\CIMV2`.
    pub fn connect() -> Result<Wmi, String> {
        unsafe {
            // S_FALSE (already initialised on this thread) is fine
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() {
                return Err(format!("COM: {hr}"));
            }
            // may already be set by the process; that is fine too
            let _ = CoInitializeSecurity(None, -1, None, None, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE, None);
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("WbemLocator: {e}"))?;
            let services = locator
                .ConnectServer(&BSTR::from("ROOT\\CIMV2"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)
                .map_err(|e| format!("ConnectServer: {e}"))?;
            CoSetProxyBlanket(&services, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, None, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE).map_err(|e| format!("CoSetProxyBlanket: {e}"))?;
            Ok(Wmi { services })
        }
    }

    /// `SELECT <properties> FROM <class>`: one JSON object per instance.
    pub fn query(&self, class: &str, properties: &[&str]) -> Result<Vec<Value>, String> {
        let wql = format!("SELECT {} FROM {class}", properties.join(", "));
        let mut out = Vec::new();
        unsafe {
            let rows = self
                .services
                .ExecQuery(&BSTR::from("WQL"), &BSTR::from(wql.as_str()), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None)
                .map_err(|e| format!("{class}: {e}"))?;
            loop {
                let mut objects = [None];
                let mut returned = 0u32;
                let hr = rows.Next(WBEM_INFINITE, &mut objects, &mut returned);
                if hr.is_err() {
                    return Err(format!("{class}: {hr}"));
                }
                if returned == 0 {
                    break;
                }
                let Some(obj) = objects[0].take() else { break };
                let mut map = serde_json::Map::new();
                for name in properties {
                    let mut v = VARIANT::default();
                    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                    obj.Get(windows::core::PCWSTR(wide.as_ptr()), 0, &mut v, None, None).map_err(|e| format!("{class}.{name}: {e}"))?;
                    map.insert(name.to_string(), variant_to_json(&v));
                    let _ = VariantClear(&mut v);
                }
                out.push(Value::Object(map));
            }
        }
        Ok(out)
    }
}

/// A WMI value as JSON. WMI hands 64-bit numbers over as text; they stay
/// text here, and the collector that knows the property turns them into
/// numbers.
pub fn variant_to_json(v: &VARIANT) -> Value {
    unsafe {
        let raw = &*v.Anonymous.Anonymous;
        let vt = raw.vt;
        if vt == VT_EMPTY || vt == VT_NULL {
            return Value::Null;
        }
        if vt.0 & VT_ARRAY.0 != 0 {
            let psa = raw.Anonymous.parray as *const windows::Win32::System::Com::SAFEARRAY;
            let (Ok(lo), Ok(hi)) = (SafeArrayGetLBound(psa, 1), SafeArrayGetUBound(psa, 1)) else { return Value::Null };
            let element = VARENUM(vt.0 & !VT_ARRAY.0);
            let mut items = Vec::new();
            for i in lo..=hi {
                if element == VT_BSTR {
                    let mut b: BSTR = BSTR::new();
                    if SafeArrayGetElement(psa, &i, &mut b as *mut BSTR as *mut _).is_ok() {
                        items.push(Value::String(b.to_string()));
                    }
                } else if element == VT_I4 || element == VT_UI4 || element == VT_I2 || element == VT_UI2 {
                    let mut n: i32 = 0;
                    if SafeArrayGetElement(psa, &i, &mut n as *mut i32 as *mut _).is_ok() {
                        items.push(Value::from(n));
                    }
                }
            }
            return Value::Array(items);
        }
        let inner = &raw.Anonymous;
        match vt {
            x if x == VT_BSTR => Value::String(inner.bstrVal.to_string()),
            x if x == VT_BOOL => Value::Bool(inner.boolVal.as_bool()),
            x if x == VT_I4 => Value::from(inner.lVal),
            x if x == VT_UI4 => Value::from(inner.ulVal),
            x if x == VT_I2 => Value::from(inner.iVal),
            x if x == VT_UI2 => Value::from(inner.uiVal),
            x if x == VT_UI1 => Value::from(inner.bVal),
            x if x == VT_I8 => Value::from(inner.llVal),
            x if x == VT_UI8 => Value::from(inner.ullVal),
            x if x == VT_R8 => Value::from(inner.dblVal),
            x if x == VT_R4 => Value::from(inner.fltVal),
            other => Value::String(format!("(variant type {})", other.0)),
        }
    }
}
