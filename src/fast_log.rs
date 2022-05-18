use std::sync::atomic::{AtomicI32, Ordering};
use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::appender::{Command, FastLogRecord};
use crate::error::LogError;
use crate::filter::{Filter};
use std::result::Result::Ok;
use std::time::{SystemTime};
use std::sync::Arc;
use crossbeam_channel::TryRecvError;
use crossbeam_utils::sync::WaitGroup;
use once_cell::sync::{Lazy, OnceCell};
use crate::{chan, Receiver, Sender, spawn};
use crate::config::Config;

pub static LOG_SENDER: Lazy<LoggerSender> = Lazy::new(|| {
    LoggerSender::new_def()
});

pub struct LoggerSender {
    pub filter: OnceCell<Box<dyn Filter>>,
    pub send: Sender<FastLogRecord>,
    pub recv: Receiver<FastLogRecord>,
}

impl LoggerSender {
    pub fn new_def() -> Self {
        let (s, r) = chan();
        LoggerSender {
            filter: OnceCell::new(),
            send: s,
            recv: r,
        }
    }
    pub fn set_filter(&self, f: Box<dyn Filter>) {
        self.filter.get_or_init(|| f);
        self.filter.get();
    }
}

fn set_log(level: log::LevelFilter, filter: Box<dyn Filter>) {
    LOGGER.set_level(level);
    LOG_SENDER.set_filter(filter);
}

pub struct Logger {
    level: AtomicI32,
}

impl Logger {
    pub fn set_level(&self, level: LevelFilter) {
        self.level
            .swap(level as i32, std::sync::atomic::Ordering::Relaxed);
        log::set_max_level(level);
    }

    pub fn get_level(&self) -> LevelFilter {
        match self.level.load(std::sync::atomic::Ordering::Relaxed) {
            0 => LevelFilter::Off,
            1 => LevelFilter::Error,
            2 => LevelFilter::Warn,
            3 => LevelFilter::Info,
            4 => LevelFilter::Debug,
            5 => LevelFilter::Trace,
            _ => panic!("error log level!"),
        }
    }

    pub fn wait(&self) {
        self.flush();
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.get_level()
    }
    fn log(&self, record: &Record) {
        //send
        let f = LOG_SENDER.filter.get();
        if f.is_some() {
            if !f.as_ref().unwrap().filter(record) {
                let fast_log_record = FastLogRecord {
                    command: Command::CommandRecord,
                    level: record.level(),
                    target: record.metadata().target().to_string(),
                    args: record.args().to_string(),
                    module_path: record.module_path().unwrap_or_default().to_string(),
                    file: record.file().unwrap_or_default().to_string(),
                    line: record.line().clone(),
                    now: SystemTime::now(),
                    formated: String::new(),
                };
                LOG_SENDER.send.send(fast_log_record);
            }
        }
    }
    fn flush(&self) {
        match flush() {
            Ok(v) => {
                v.wait();
            }
            Err(_) => {}
        }
    }
}

static LOGGER: Logger = Logger {
    level: AtomicI32::new(1),
};


pub fn init(config: Config) -> Result<&'static Logger, LogError> {
    if config.appenders.is_empty() {
        return Err(LogError::from("[fast_log] appenders can not be empty!"));
    }
    set_log(config.level.clone(), config.filter);
    //main recv data
    let appenders = config.appenders;
    let format = config.format;
    let level = config.level;
    std::thread::spawn(move || {
        let mut recever_vec = vec![];
        let mut sender_vec: Vec<Sender<Arc<Vec<FastLogRecord>>>> = vec![];
        for a in appenders {
            let (s, r) = chan();
            sender_vec.push(s);
            recever_vec.push((r, a));
        }
        for (recever, appender) in recever_vec {
            spawn(move || {
                let mut exit = false;
                loop {
                    //batch fetch
                    if let Ok(msg) = recever.recv() {
                        appender.do_logs(msg.as_ref());
                        for x in msg.iter() {
                            match x.command {
                                Command::CommandRecord => {}
                                Command::CommandExit => {
                                    exit = true;
                                    continue;
                                }
                                Command::CommandFlush(_) => {
                                    appender.flush();
                                    continue;
                                }
                            }
                        }
                    }
                    if exit {
                        break;
                    }
                }
            });
        }
        loop {
            //recv
            let data = LOG_SENDER.recv.recv();
            if let Ok(mut data) = data {
                let mut remain;
                if LOG_SENDER.recv.len() > 0 {
                    remain = recv_all(&LOG_SENDER.recv);
                    remain.insert(0, data);
                } else {
                    remain = vec![data];
                }
                let mut exit = false;
                for x in &mut remain {
                    x.formated = format.do_format(x);
                    match x.command {
                        Command::CommandRecord => {}
                        Command::CommandExit => {
                            exit = true;
                        }
                        Command::CommandFlush(_) => {}
                    }
                }
                let data = Arc::new(remain);
                for x in &sender_vec {
                    x.send(data.clone());
                }
                if exit {
                    break;
                }
            }
        }
    });
    let r = log::set_logger(&LOGGER).map(|()| log::set_max_level(level));
    if r.is_err() {
        return Err(LogError::from(r.err().unwrap()));
    } else {
        return Ok(&LOGGER);
    }
}

fn recv_all<T>(recver: &Receiver<T>) -> Vec<T> {
    let mut data = Vec::with_capacity(recver.len());
    loop {
        match recver.try_recv() {
            Ok(v) => {
                data.push(v);
            }
            Err(_) => {
                break;
            }
        }
    }
    data
}


pub fn exit() -> Result<(), LogError> {
    let fast_log_record = FastLogRecord {
        command: Command::CommandExit,
        level: log::Level::Info,
        target: String::new(),
        args: String::new(),
        module_path: String::new(),
        file: String::new(),
        line: None,
        now: SystemTime::now(),
        formated: String::new(),
    };
    let result = LOG_SENDER.send.send(fast_log_record);
    match result {
        Ok(()) => {
            return Ok(());
        }
        _ => {}
    }
    return Err(LogError::E("[fast_log] exit fail!".to_string()));
}


pub fn flush() -> Result<WaitGroup, LogError> {
    let wg = WaitGroup::new();
    let fast_log_record = FastLogRecord {
        command: Command::CommandFlush(wg.clone()),
        level: log::Level::Info,
        target: String::new(),
        args: String::new(),
        module_path: String::new(),
        file: String::new(),
        line: None,
        now: SystemTime::now(),
        formated: String::new(),
    };
    let result = LOG_SENDER.send.send(fast_log_record);
    match result {
        Ok(()) => {
            return Ok(wg);
        }
        _ => {}
    }
    return Err(LogError::E("[fast_log] flush fail!".to_string()));
}
