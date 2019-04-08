/************************************************

   File: rato:net_events.rs:NetEvents
   Author: ytr289
   Date: 2019-03-26:09:33
   LICENSE: Apache 2.0

**************************************************/
use hashbrown::HashMap;

use crate::conn::Conn;

///
/// NetEvents
///
pub trait NetEvents {
    fn event_opened(&self, id: usize, conn: &mut Conn) -> (Vec<u8>, bool);
    fn event_closed(&self, id: usize, conn: &mut Conn) -> Result<bool, String>;
    fn event_data(
        &self,
        id: usize,
        conn_tags: &mut HashMap<String, String>,
        input_buffer: &mut Vec<u8>,
        output_buffer: &mut Vec<u8>,
    ) -> bool;
}