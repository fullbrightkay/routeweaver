use crate::{discover::Discovery, error::RouteWeaverError};
use bluer::{
    adv::Advertisement,
    monitor::{
        data_type::COMPLETE_LIST_128_BIT_SERVICE_CLASS_UUIDS, Monitor, MonitorEvent, Pattern,
    },
};
use futures_util::{future::pending, stream::select_all, Stream, StreamExt};
use routeweaver_common::{Address, Peer, Protocol};
use std::collections::BTreeSet;
use uuid::Uuid;

const DEFAULT_UUID: Uuid = Uuid::from_u128(0xf28798f4_7982_452d_95d7_9927699ded9a);

pub struct BluetoothDiscovery {
    session: bluer::Session,
}

impl BluetoothDiscovery {
    pub async fn new() -> Result<Self, RouteWeaverError> {
        let session = bluer::Session::new()
            .await
            .map_err(|_| RouteWeaverError::ConnectionFailed)?;

        Ok(Self { session })
    }
}

impl Discovery for BluetoothDiscovery {
    const ID: &'static str = "bluetooth-passive";

    async fn announce(&self) -> Result<(), RouteWeaverError> {
        let advertisement = Advertisement {
            advertisement_type: bluer::adv::Type::Peripheral,
            service_uuids: BTreeSet::from_iter([DEFAULT_UUID]),
            discoverable: Some(true),
            local_name: Some("routeweaver".to_owned()),
            ..Default::default()
        };

        for adapter_name in self
            .session
            .adapter_names()
            .await
            .map_err(|_| RouteWeaverError::ConnectionFailed)?
        {
            if let Ok(adapter) = self.session.adapter(&adapter_name) {
                let handle = adapter
                    .advertise(advertisement.clone())
                    .await
                    .map_err(|_| RouteWeaverError::ConnectionFailed)?;

                // Wait forever so the advertisement handle is never dropped
                pending::<()>().await;
                // Never actually called
                drop(handle);
            }
        }

        Ok(())
    }

    async fn discover(
        &self,
    ) -> Result<impl Stream<Item = Result<Peer, RouteWeaverError>>, RouteWeaverError> {
        let adapter_names = self
            .session
            .adapter_names()
            .await
            .map_err(|_| RouteWeaverError::ConnectionFailed)?;

        let mut monitor_handles = Vec::with_capacity(adapter_names.len());

        for adapter_name in adapter_names {
            tracing::debug!("Starting monitor on bluetooth adapter {}", adapter_name);

            if let Ok(adapter) = self.session.adapter(&adapter_name) {
                let monitor = adapter.monitor().await.unwrap();
                let monitor_handle = monitor
                    .register(Monitor {
                        monitor_type: bluer::monitor::Type::OrPatterns,
                        patterns: Some(vec![Pattern::new(
                            COMPLETE_LIST_128_BIT_SERVICE_CLASS_UUIDS,
                            0,
                            DEFAULT_UUID.as_bytes(),
                        )]),
                        ..Default::default()
                    })
                    .await
                    .unwrap();
                monitor_handles.push(monitor_handle);
            }
        }

        let discovered_devices = select_all(monitor_handles)
            .filter_map(|monitor_event| async move {
                match monitor_event {
                    MonitorEvent::DeviceFound(device_id) => {
                        let address = Address::Bluetooth(device_id.device.0);

                        tracing::debug!(
                            "Discovered device {} on bluetooth adapter {}",
                            address,
                            device_id.adapter
                        );

                        Some(Peer {
                            protocol: Protocol::Bluetooth,
                            address,
                            port: None,
                        })
                    }
                    _ => None,
                }
            })
            .map(Ok);

        Ok(discovered_devices)
    }
}
