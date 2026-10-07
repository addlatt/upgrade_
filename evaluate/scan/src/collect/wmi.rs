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

/// The namespaces the scanner reads.
pub const CIMV2: &str = "ROOT\\CIMV2";
pub const STORAGE: &str = "ROOT\\Microsoft\\Windows\\Storage";
pub const WMI_ROOT: &str = "ROOT\\WMI";
pub const BITLOCKER: &str = "ROOT\\CIMV2\\Security\\MicrosoftVolumeEncryption";

impl Wmi {
    /// Connect to one namespace.
    pub fn connect(namespace: &str) -> Result<Wmi, String> {
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
                .ConnectServer(&BSTR::from(namespace), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)
                .map_err(|e| format!("ConnectServer: {e}"))?;
            CoSetProxyBlanket(&services, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, None, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE).map_err(|e| format!("CoSetProxyBlanket: {e}"))?;
            Ok(Wmi { services })
        }
    }

    /// `SELECT <properties> FROM <class>`: one JSON object per instance.
    pub fn query(&self, class: &str, properties: &[&str]) -> Result<Vec<Value>, String> {
        self.wql(&format!("SELECT {} FROM {class}", properties.join(", ")), properties)
    }

    /// `SELECT <properties> FROM <class> WHERE <condition>`.
    pub fn query_where(&self, class: &str, properties: &[&str], condition: &str) -> Result<Vec<Value>, String> {
        self.wql(&format!("SELECT {} FROM {class} WHERE {condition}", properties.join(", ")), properties)
    }

    /// Any WQL. Each row also carries `__RELPATH`, the object's own address,
    /// so a method can be called on it. The address is only complete when
    /// the class's key property (`ObjectId` in the Storage namespace) is
    /// among the properties asked for.
    pub fn wql(&self, wql: &str, properties: &[&str]) -> Result<Vec<Value>, String> {
        let class = wql.split(" FROM ").nth(1).unwrap_or(wql).split(' ').next().unwrap_or(wql).to_string();
        let mut out = Vec::new();
        unsafe {
            let rows = self
                .services
                .ExecQuery(&BSTR::from("WQL"), &BSTR::from(wql), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None)
                .map_err(|e| wmi_error(&e))?;
            loop {
                let mut objects = [None];
                let mut returned = 0u32;
                let hr = rows.Next(WBEM_INFINITE, &mut objects, &mut returned);
                if hr.is_err() {
                    return Err(wmi_error(&windows::core::Error::from_hresult(hr)));
                }
                if returned == 0 {
                    break;
                }
                let Some(obj) = objects[0].take() else { break };
                let mut map = serde_json::Map::new();
                for sys in ["__PATH", "__RELPATH"] {
                    let mut path = VARIANT::default();
                    let wide: Vec<u16> = sys.encode_utf16().chain(std::iter::once(0)).collect();
                    if obj.Get(windows::core::PCWSTR(wide.as_ptr()), 0, &mut path, None, None).is_ok() {
                        map.insert(sys.to_string(), variant_to_json(&path));
                        let _ = VariantClear(&mut path);
                    }
                }
                for name in properties {
                    let mut v = VARIANT::default();
                    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                    obj.Get(windows::core::PCWSTR(wide.as_ptr()), 0, &mut v, None, None).map_err(|e| format!("{class}.{name}: {}", wmi_error(&e)))?;
                    map.insert(name.to_string(), variant_to_json(&v));
                    let _ = VariantClear(&mut v);
                }
                out.push(Value::Object(map));
            }
        }
        Ok(out)
    }
}

impl Wmi {
    /// Call a method on one object (by its `__PATH`) with no input, and
    /// return the output parameters as JSON (`ReturnValue` among them).
    pub fn call(&self, class: &str, path: &str, method: &str, out_names: &[&str]) -> Result<Value, String> {
        self.call_with(class, path, method, &[], out_names)
    }

    /// A method call with input parameters (booleans and whole numbers).
    pub fn call_with(&self, class: &str, path: &str, method: &str, inputs: &[(&str, Value)], out_names: &[&str]) -> Result<Value, String> {
        use windows::Win32::System::Wmi::IWbemClassObject;
        unsafe {
            // an empty input object, as Invoke-CimMethod sends one
            let mut class_obj: Option<IWbemClassObject> = None;
            self.services.GetObject(&BSTR::from(class), windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class_obj), None).map_err(|e| wmi_error(&e))?;
            let class_obj = class_obj.ok_or("no class object")?;
            let mut in_class: Option<IWbemClassObject> = None;
            let wide: Vec<u16> = method.encode_utf16().chain(std::iter::once(0)).collect();
            class_obj.GetMethod(windows::core::PCWSTR(wide.as_ptr()), 0, &mut in_class, std::ptr::null_mut()).map_err(|e| wmi_error(&e))?;
            let in_params = match in_class {
                Some(c) => Some(c.SpawnInstance(0).map_err(|e| wmi_error(&e))?),
                None => None,
            };
            if let Some(params) = &in_params {
                for (name, value) in inputs {
                    let v = variant_from_json(value).ok_or_else(|| format!("{method}: an input of a kind this cannot send ({name})"))?;
                    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                    params.Put(windows::core::PCWSTR(wide.as_ptr()), 0, &v, 0).map_err(|e| format!("{method}.{name}: {}", wmi_error(&e)))?;
                }
            }
            let mut out: Option<IWbemClassObject> = None;
            self.services
                .ExecMethod(&BSTR::from(path), &BSTR::from(method), windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(0), None, in_params.as_ref(), Some(&mut out), None)
                .map_err(|e| wmi_error(&e))?;
            let Some(obj) = out else { return Err(format!("{method}: no output")) };
            let mut map = serde_json::Map::new();
            for name in out_names {
                let mut v = VARIANT::default();
                let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                if obj.Get(windows::core::PCWSTR(wide.as_ptr()), 0, &mut v, None, None).is_ok() {
                    map.insert(name.to_string(), variant_to_json(&v));
                    let _ = VariantClear(&mut v);
                }
            }
            Ok(Value::Object(map))
        }
    }
}

impl Wmi {
    /// One object by its path, with the properties named.
    pub fn get_object(&self, path: &str, properties: &[&str]) -> Result<Value, String> {
        use windows::Win32::System::Wmi::IWbemClassObject;
        unsafe {
            let mut obj: Option<IWbemClassObject> = None;
            self.services.GetObject(&BSTR::from(path), windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut obj), None).map_err(|e| wmi_error(&e))?;
            let obj = obj.ok_or("no object")?;
            let mut map = serde_json::Map::new();
            for name in properties {
                let mut v = VARIANT::default();
                let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                if obj.Get(windows::core::PCWSTR(wide.as_ptr()), 0, &mut v, None, None).is_ok() {
                    map.insert(name.to_string(), variant_to_json(&v));
                    let _ = VariantClear(&mut v);
                }
            }
            Ok(Value::Object(map))
        }
    }

    /// Try a method call several ways and tell what each said (a debugging
    /// aid for `--try-wmi`).
    pub fn try_method(&self, class: &str, condition: &str, method: &str) -> Vec<String> {
        let mut out = Vec::new();
        let rows = match self.query_where(class, &["ObjectId"], condition) {
            Ok(r) => r,
            Err(e) => return vec![format!("query: {e}")],
        };
        let Some(row) = rows.first() else { return vec!["no instance".into()] };
        for key in ["__RELPATH", "__PATH"] {
            let path = row[key].as_str().unwrap_or("").to_string();
            let r = self.call(class, &path, method, &["ReturnValue", "SizeMin"]);
            out.push(format!("{key} = {path}
      -> {r:?}"));
        }
        out
    }
}

impl Wmi {
    /// Change one property of an existing object and write it back
    /// (`Set-CimInstance -Property @{ name = value }`).
    pub fn put_property(&self, path: &str, name: &str, value: &Value) -> Result<(), String> {
        use windows::Win32::System::Wmi::{IWbemClassObject, WBEM_FLAG_UPDATE_ONLY};
        unsafe {
            let mut obj: Option<IWbemClassObject> = None;
            self.services.GetObject(&BSTR::from(path), windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut obj), None).map_err(|e| wmi_error(&e))?;
            let obj = obj.ok_or("no object")?;
            let v = variant_from_json(value).ok_or_else(|| format!("{name}: a value of a kind this cannot send"))?;
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            obj.Put(windows::core::PCWSTR(wide.as_ptr()), 0, &v, 0).map_err(|e| format!("{name}: {}", wmi_error(&e)))?;
            self.services.PutInstance(&obj, windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_UPDATE_ONLY.0), None, None).map_err(|e| wmi_error(&e))
        }
    }

    /// Remove one object by its path (`Remove-CimInstance`).
    pub fn delete_instance(&self, path: &str) -> Result<(), String> {
        unsafe { self.services.DeleteInstance(&BSTR::from(path), windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(0), None, None).map_err(|e| wmi_error(&e)) }
    }

    /// Make one new object of a class with the properties given
    /// (`New-CimInstance -ClassName ... -Property @{...}`).
    pub fn create_instance(&self, class: &str, properties: &[(&str, Value)]) -> Result<(), String> {
        use windows::Win32::System::Wmi::{IWbemClassObject, WBEM_FLAG_CREATE_ONLY};
        unsafe {
            let mut class_obj: Option<IWbemClassObject> = None;
            self.services.GetObject(&BSTR::from(class), windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class_obj), None).map_err(|e| wmi_error(&e))?;
            let inst = class_obj.ok_or("no class object")?.SpawnInstance(0).map_err(|e| wmi_error(&e))?;
            for (name, value) in properties {
                let v = variant_from_json(value).ok_or_else(|| format!("{name}: a value of a kind this cannot send"))?;
                let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                inst.Put(windows::core::PCWSTR(wide.as_ptr()), 0, &v, 0).map_err(|e| format!("{name}: {}", wmi_error(&e)))?;
            }
            self.services.PutInstance(&inst, windows::Win32::System::Wmi::WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_CREATE_ONLY.0), None, None).map_err(|e| wmi_error(&e))
        }
    }
}

/// A JSON value as the VARIANT WMI takes for it: a boolean, a whole number
/// that fits 32 bits, a larger whole number as text (how WMI carries uint64),
/// or a string.
pub fn variant_from_json(value: &Value) -> Option<VARIANT> {
    unsafe {
        let mut v = VARIANT::default();
        let inner = &mut *v.Anonymous.Anonymous;
        match value {
            Value::Bool(b) => {
                inner.vt = VT_BOOL;
                inner.Anonymous.boolVal = (*b).into();
            }
            Value::Number(n) => match n.as_i64() {
                Some(i) if i >= i32::MIN as i64 && i <= i32::MAX as i64 => {
                    inner.vt = VT_I4;
                    inner.Anonymous.lVal = i as i32;
                }
                _ => {
                    inner.vt = VT_BSTR;
                    inner.Anonymous.bstrVal = std::mem::ManuallyDrop::new(BSTR::from(n.to_string()));
                }
            },
            Value::String(s) => {
                inner.vt = VT_BSTR;
                inner.Anonymous.bstrVal = std::mem::ManuallyDrop::new(BSTR::from(s.as_str()));
            }
            _ => return None,
        }
        Some(v)
    }
}

/// A WMI failure in the words PowerShell's CIM cmdlets use for it.
pub fn wmi_error(e: &windows::core::Error) -> String {
    let code = e.code().0 as u32;
    // the message text of a WBEM code is not in the system's tables
    let fallback = || {
        let m = e.message().trim().to_string();
        if m.is_empty() || m.starts_with("0x") { format!("WMI error {code:#010x} [{e:?}]") } else { m }
    };
    match code {
        0x8004100C => "Not supported".to_string(),
        0x80041003 => "Access denied".to_string(),
        0x80041010 => "Invalid class".to_string(),
        0x8004100E => "Invalid namespace".to_string(),
        0x80041002 => "Not found".to_string(),
        0x80041017 => "Invalid query".to_string(),
        0x80041008 => "Invalid parameter".to_string(),
        0x8004103A => "Invalid object path".to_string(),
        0x80041006 => "Out of memory".to_string(),
        0x80041013 => "Provider load failure".to_string(),
        0x80041014 => "Initialization failure".to_string(),
        0x80041032 => "Call cancelled".to_string(),
        _ => fallback(),
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
                } else if element == VT_I4 || element == VT_UI4 {
                    let mut n: i32 = 0;
                    if SafeArrayGetElement(psa, &i, &mut n as *mut i32 as *mut _).is_ok() {
                        items.push(Value::from(n));
                    }
                } else if element == VT_I2 || element == VT_UI2 {
                    let mut n: i16 = 0;
                    if SafeArrayGetElement(psa, &i, &mut n as *mut i16 as *mut _).is_ok() {
                        items.push(Value::from(n as i32));
                    }
                } else if element == VT_UI1 {
                    let mut n: u8 = 0;
                    if SafeArrayGetElement(psa, &i, &mut n as *mut u8 as *mut _).is_ok() {
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
