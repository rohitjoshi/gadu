/************************************************

   File Name: gadu:lib
   Author: Rohit Joshi <rohit.c.joshi@gmail.com>
   Date: 2019-02-17:15:15
   License: Apache 2.0

**************************************************/
//#![feature(await_macro, async_await, futures_api)]
//#[macro_use]
//extern crate tokio;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

pub mod net_events;
pub mod config;
pub mod conn;
pub mod events;
pub mod network_server;
pub mod server;

#[cfg(test)]
mod tests {
    use std::thread;

    use crate::conn::Conn;
    use crate::server::Server;

    use super::*;

    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
