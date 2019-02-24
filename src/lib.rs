/************************************************

   File Name: gadu:lib
   Author: Rohit Joshi <rohit.c.joshi@gmail.com>
   Date: 2019-02-17:15:15
   License: Apache 2.0

**************************************************/
#![feature(integer_atomics)]
#[macro_use]
extern crate log;
#[macro_use] extern crate serde_derive;

pub mod conn;
pub mod server;
pub mod events;
pub mod config;
#[cfg(test)]
mod tests {

    use super::*;
    use crate::conn::Conn;
    use crate::server::Server;
    use std::thread;
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }



}
