//! eave_glass_v1's client side, generated from `protocol/eave-glass-v1.xml` at the repository's
//! root, which hypr-eave generates its side from too.

#![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#![allow(non_upper_case_globals, non_snake_case, unused_imports)]
#![allow(missing_docs, clippy::all)]

use wayland_client;
use wayland_client::backend as wayland_backend;
use wayland_client::protocol::*;

pub mod __interfaces {
    use wayland_client::backend as wayland_backend;
    use wayland_client::protocol::__interfaces::*;
    wayland_scanner::generate_interfaces!("../../protocol/eave-glass-v1.xml");
}
use self::__interfaces::*;

wayland_scanner::generate_client_code!("../../protocol/eave-glass-v1.xml");
