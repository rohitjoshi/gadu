/************************************************

   File: gadu:network_server:NetworkServer
   Author: Rohit Joshi
   Date: 2019-03-03:20:48
   LICENSE: Apache 2.0

**************************************************/
use crate::config::NetworkServerConfig;
use crate::conn::Conn;
use crate::events::{ConnEventHandler, NetEvents, ServerEventHandler};
use crossbeam::thread::Scope;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
///
/// NetworkServer
///
pub struct NetworkServer {
    //config : NetworkServerConfig,
    server_event_handler: ServerEventHandler,
    conn_handlers: Vec<Arc<ConnEventHandler>>,
    pub shutdown: Arc<AtomicBool>,
    //db_controller: Arc<DbController>,
    round_robin_counter: AtomicUsize,
}

/// implementation
impl NetworkServer {
    ///
    /// new() : returns new object
    ///
    pub fn new(
        conf: &NetworkServerConfig,
        shutdown: Arc<AtomicBool>,
    ) -> Result<NetworkServer, String> {
        let server_event_handler =
            ServerEventHandler::new(conf.server_id, conf.num_threads, &conf.server_config)?;
        let mut conn_handlers = Vec::with_capacity(conf.num_threads);
        for i in 0..conf.num_threads {
            debug!("Initializing NetworkServer::ConnEventHandler {}", i);
            let handler = ConnEventHandler::new()?;
            conn_handlers.push(Arc::new(handler));
        }
        Ok(NetworkServer {
            //config : conf.clone(),
            server_event_handler,
            conn_handlers,
            shutdown,
            round_robin_counter: AtomicUsize::new(0),
        })
    }
    fn add_connection(&self, id: usize, conn: Conn) -> Result<(), String> {
        debug!("add_connection with id:{}", id);

        let mut index = self.round_robin_counter.fetch_add(1, Ordering::SeqCst);
        if index >= self.conn_handlers.len() {
            self.round_robin_counter.store(0, Ordering::SeqCst);
            index = 0;
        }
        self.conn_handlers[index].add_connection(id, conn)
    }

    fn server_loop(&self, net_event_handler: Arc<NetEvents>) {
        let mut id = self.server_event_handler.server_id;
        info!("Waiting for connection...");
        while !self.shutdown.load(Ordering::SeqCst) {
            id += 1;
            if let Ok(mut conn) = self.server_event_handler.server.accept_connection() {
                let (output, close) = net_event_handler.event_opened(id, &mut conn);
                if close {
                    conn.close = close;
                } else if !output.is_empty() {
                    conn.output = output;
                    conn.reg_write = true;
                }

                if let Err(e) = self.add_connection(id, conn) {
                    error!("Failed to add connect with id: {}. Error:{:?}", id, e);
                }

                continue;
            } else {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }

    pub fn run_loop<T>(scope: &Scope, network_server: Arc<NetworkServer>, net_event_handler: Arc<T>)
    where
        T: NetEvents + 'static + Sync + Send + Sized,
    {
        for conn_handler in network_server.conn_handlers.iter() {
            let ev = net_event_handler.clone(); //kanudo.network_controller.clone();
            let c = conn_handler.clone();
            let shutdown = network_server.shutdown.clone();
            info!("Starting connection handler child loop");
            scope.spawn(move |_| c.child_loop(ev, shutdown));
        }

        info!("Starting kanudo server loop");
        scope.spawn(move |_| network_server.server_loop(net_event_handler));
    }
}