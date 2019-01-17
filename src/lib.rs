#[macro_use]
extern crate log;

pub mod conn;
pub mod server;

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
