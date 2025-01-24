use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        transport_tcp: {
            all(
                feature = "transport-tcp",
                any(
                    target_os = "linux",
                    target_os = "macos",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "windows"
                )
            )
        },
        transport_udp: {
            all(
                feature = "transport-udp",
                any(
                    target_os = "linux",
                    target_os = "macos",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "windows"
                )
            )
        },
        transport_ws: {
            all(
                feature = "transport-ws",
                any(
                    target_os = "linux",
                    target_os = "macos",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "windows"
                )
            )
        },
        transport_wss: {
            all(
                feature = "transport-wss",
                any(
                    target_os = "linux",
                    target_os = "macos",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "windows"
                )
            )
        },
        transport_bluetooth: {
            all(
                feature = "transport-bluetooth",
                any(
                    target_os = "linux"
                )
            )
        },
        discovery_bluetooth_passive: {
            all(
                feature = "discovery-bluetooth-passive",
                any(
                    target_os = "linux"
                )
            )
        },
        discovery_udp_multicast: {
            all(
                feature = "discovery-udp-multicast",
                any(
                    target_os = "linux",
                    target_os = "macos",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "windows"
                )
            )
        }
    }
}
