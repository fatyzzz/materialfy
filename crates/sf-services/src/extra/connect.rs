//! Список устройств Spotify Connect для меню (spotifast ui/devices.rs: receiver_is_listed, name_devices):
//! устройства Web API + этот ПК + приёмники из локальной сети, которых аккаунт ещё не знает.
use std::collections::HashSet;

use md3_model::DeviceItem;
use sf_player::engine::zeroconf::Receiver;

pub const SOURCE_API: &str = "api";
pub const SOURCE_ZEROCONF: &str = "zeroconf";

/// Приёмник уже есть в списке Web API (по id, без id — по имени).
pub fn receiver_is_listed(receiver: &Receiver, devices: &[DeviceItem]) -> bool {
    devices.iter().any(|d| match receiver.device_id.as_deref() {
        Some(id) => d.id == id,
        None => d.name == receiver.name,
    })
}

/// Имя из mDNS точнее, чем у Web API (там часто модель вместо имени, данного пользователем).
pub fn name_devices(devices: &mut [DeviceItem], receivers: &[Receiver]) {
    for device in devices.iter_mut().filter(|d| !d.id.is_empty()) {
        if let Some(receiver) = receivers.iter().find(|r| r.device_id.as_deref() == Some(device.id.as_str())) {
            device.name.clone_from(&receiver.name);
        }
    }
}

/// id строки приёмника в списке (у приёмника без deviceID — по имени).
pub fn receiver_row_id(receiver: &Receiver) -> String {
    match receiver.device_id.as_deref().filter(|id| !id.is_empty()) {
        Some(id) => id.to_owned(),
        None => format!("zeroconf:{}", receiver.name),
    }
}

pub fn receiver_item(receiver: &Receiver) -> DeviceItem {
    DeviceItem {
        id: receiver_row_id(receiver),
        name: receiver.name.clone(),
        kind: "speaker".into(),
        volume: -1,
        source: SOURCE_ZEROCONF.into(),
        ..Default::default()
    }
}

/// Что известно об этом ПК, когда движок готов.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThisDevice {
    pub id: String,
    pub name: String,
    /// 0..100
    pub volume: i32,
}

/// Итоговый список: активное первым, затем этот ПК, затем остальные (порядок Web API); без повторов id.
/// `active_id` — устройство, которое показывает плеер (оптимистично, после переноса), "" — нигде.
pub fn merge(
    api: &[DeviceItem],
    receivers: &[Receiver],
    this: Option<&ThisDevice>,
    active_id: &str,
) -> Vec<DeviceItem> {
    let mut devices: Vec<DeviceItem> = api.to_vec();
    for d in &mut devices {
        if d.source.is_empty() {
            d.source = SOURCE_API.into();
        }
    }
    if let Some(this) = this.filter(|t| !t.id.is_empty()) {
        match devices.iter_mut().find(|d| d.id == this.id) {
            Some(d) => d.is_this_computer = true,
            None => devices.insert(
                0,
                DeviceItem {
                    id: this.id.clone(),
                    name: this.name.clone(),
                    kind: "computer".into(),
                    is_this_computer: true,
                    supports_volume: true,
                    volume: this.volume,
                    source: SOURCE_API.into(),
                    ..Default::default()
                },
            ),
        }
    }
    name_devices(&mut devices, receivers);
    let mut seen = HashSet::new();
    devices.retain(|d| d.id.is_empty() || seen.insert(d.id.clone()));
    for d in &mut devices {
        d.is_active = !active_id.is_empty() && d.id == active_id;
    }
    // стабильная сортировка: активное, этот ПК, остальные
    devices.sort_by_key(|d| (!d.is_active, !d.is_this_computer));
    devices.extend(receivers.iter().filter(|r| !receiver_is_listed(r, api)).map(receiver_item).filter(|r| {
        let fresh = !seen.contains(&r.id);
        seen.insert(r.id.clone());
        fresh
    }));
    devices
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn device(id: &str, name: &str) -> DeviceItem {
        DeviceItem { id: id.into(), name: name.into(), kind: "speaker".into(), volume: 50, ..Default::default() }
    }

    fn receiver(name: &str, id: Option<&str>) -> Receiver {
        Receiver {
            name: name.into(),
            device_id: id.map(Into::into),
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 80,
            path: "/zc".into(),
        }
    }

    #[test]
    fn listed_receivers_are_matched_by_id_then_name() {
        let api = [device("a", "Кухня"), device("b", "Model X")];
        assert!(receiver_is_listed(&receiver("Другое", Some("a")), &api));
        assert!(receiver_is_listed(&receiver("Кухня", None), &api));
        assert!(!receiver_is_listed(&receiver("Кухня", Some("zz")), &api));
    }

    #[test]
    fn receivers_rename_devices_and_unlisted_ones_are_offered() {
        let api = [device("b", "Model X")];
        let receivers = [receiver("Спальня", Some("b")), receiver("Гостиная", None)];
        let list = merge(&api, &receivers, None, "");
        let names: Vec<_> = list.iter().map(|d| (d.name.as_str(), d.source.as_str())).collect();
        assert_eq!(names, [("Спальня", "api"), ("Гостиная", "zeroconf")]);
        assert_eq!(list[1].id, "zeroconf:Гостиная");
        assert_eq!(list[1].volume, -1);
    }

    #[test]
    fn order_is_active_this_computer_then_others() {
        let api = [device("x", "X"), device("y", "Y"), device("z", "Z")];
        let this = ThisDevice { id: "me".into(), name: "Materialfy · PC".into(), volume: 40 };
        let list = merge(&api, &[], Some(&this), "z");
        let ids: Vec<_> = list.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["z", "me", "x", "y"]);
        assert!(list[0].is_active && !list[1].is_active);
        assert!(list[1].is_this_computer && list[1].supports_volume);
        assert_eq!(list[1].kind, "computer");
    }

    #[test]
    fn this_computer_listed_by_web_api_is_marked_not_duplicated() {
        let api = [device("x", "X"), device("me", "Materialfy · PC"), device("x", "X dup")];
        let this = ThisDevice { id: "me".into(), name: "ignored".into(), volume: 10 };
        let list = merge(&api, &[], Some(&this), "me");
        let ids: Vec<_> = list.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["me", "x"]);
        assert!(list[0].is_this_computer && list[0].is_active);
        assert_eq!(list[0].name, "Materialfy · PC");
    }
}
