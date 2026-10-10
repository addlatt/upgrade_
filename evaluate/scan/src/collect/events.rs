//! The Windows event log, read the way `Get-WinEvent` reads it: a channel,
//! a filter, and for each event its id, provider, time (local) and the
//! message as the provider's own text formats it.

use super::win::{utc_to_local, wide};
use crate::ps::Stamp;
use windows::core::PCWSTR;
use windows::Win32::System::EventLog::{EvtClose, EvtFormatMessage, EvtFormatMessageEvent, EvtNext, EvtOpenPublisherMetadata, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender, EvtRenderEventXml, EVT_HANDLE};

#[derive(Debug, Clone)]
pub struct Event {
    pub id: i64,
    pub provider: String,
    pub time_local: Stamp,
    pub message: String,
}

fn xml_of(event: EVT_HANDLE) -> Option<String> {
    unsafe {
        let (mut used, mut count) = (0u32, 0u32);
        let _ = EvtRender(None, event, EvtRenderEventXml.0, 0, None, &mut used, &mut count);
        if used == 0 {
            return None;
        }
        let mut buf = vec![0u16; (used as usize).div_ceil(2) + 1];
        EvtRender(None, event, EvtRenderEventXml.0, (buf.len() * 2) as u32, Some(buf.as_mut_ptr() as *mut _), &mut used, &mut count).ok()?;
        let end = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

fn message_of(provider: &str, event: EVT_HANDLE) -> String {
    unsafe {
        let p = wide(provider);
        let Ok(meta) = EvtOpenPublisherMetadata(None, PCWSTR(p.as_ptr()), PCWSTR::null(), 0, 0) else { return String::new() };
        let mut used = 0u32;
        let _ = EvtFormatMessage(Some(meta), Some(event), 0, None, EvtFormatMessageEvent.0, None, &mut used);
        let text = if used == 0 {
            String::new()
        } else {
            let mut buf = vec![0u16; used as usize + 1];
            match EvtFormatMessage(Some(meta), Some(event), 0, None, EvtFormatMessageEvent.0, Some(&mut buf), &mut used) {
                Ok(()) => {
                    let end = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
                    String::from_utf16_lossy(&buf[..end])
                }
                Err(_) => String::new(),
            }
        };
        let _ = EvtClose(meta);
        text
    }
}

/// `2026-10-06T21:30:48.1234567Z` -> a local stamp.
fn local_time(system_time: &str) -> Option<Stamp> {
    let t = system_time.get(..19)?;
    Some(utc_to_local(Stamp::parse(t)?))
}

/// The events a channel holds that match an XPath filter, newest first.
pub fn query(channel: &str, xpath: &str) -> Result<Vec<Event>, String> {
    let (c, x) = (wide(channel), wide(xpath));
    let mut out = Vec::new();
    unsafe {
        let results = EvtQuery(None, PCWSTR(c.as_ptr()), PCWSTR(x.as_ptr()), EvtQueryChannelPath.0 | EvtQueryReverseDirection.0).map_err(|e| format!("{channel}: {}", e.message()))?;
        loop {
            let mut handles = [0isize; 32];
            let mut returned = 0u32;
            if EvtNext(results, &mut handles, 5000, 0, &mut returned).is_err() || returned == 0 {
                break;
            }
            for h in &handles[..returned as usize] {
                let event = EVT_HANDLE(*h);
                if let Some(xml) = xml_of(event) {
                    if let Ok(doc) = roxmltree::Document::parse(&xml) {
                        let system = doc.root_element().children().find(|n| n.has_tag_name("System"));
                        let find = |name: &str| system.and_then(|s| s.children().find(|n| n.has_tag_name(name)));
                        let provider = find("Provider").and_then(|n| n.attribute("Name")).unwrap_or("").to_string();
                        let id = find("EventID").and_then(|n| n.text()).and_then(|t| t.trim().parse().ok()).unwrap_or(0);
                        let time = find("TimeCreated").and_then(|n| n.attribute("SystemTime")).and_then(local_time);
                        if let Some(time_local) = time {
                            let message = message_of(&provider, event);
                            out.push(Event { id, provider, time_local, message });
                        }
                    }
                }
                let _ = EvtClose(event);
            }
        }
        let _ = EvtClose(results);
    }
    Ok(out)
}

/// An XPath time clause: events of the last `seconds`.
pub fn within(seconds: i64) -> String {
    format!("TimeCreated[timediff(@SystemTime) <= {}]", seconds * 1000)
}
