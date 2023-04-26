use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Debug, Serialize, Deserialize, Default)]
pub enum WriteMode {
    #[default]
    Replace,
    Append,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    Write {
        #[serde(default)]
        mode: WriteMode,
        data: Vec<u8>,
        path: PathBuf,
    },
    Copy {
        from: PathBuf,
        to: PathBuf,
    },
    Exec {
        #[serde(default)]
        env: Vec<(String, String)>,
        exe: PathBuf,
        #[serde(default)]
        argv: Vec<String>,
        arg0: Option<String>,
        wd: Option<PathBuf>,
    },
    Listen {
        addr: SocketAddr,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Pup {
    pub id: String,
    #[serde(flatten)]
    pub cmd: Command,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Resp {
    pub id: String,
    #[serde(flatten)]
    pub res: Res,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum OutPipe {
    Out,
    Err,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Res {
    Write {},
    Copy {},
    Exec {},
    Listen { addr: SocketAddr },
    Exit { code: Option<i32> },
    Output { out_pipe: OutPipe, msg: String },
    Error { msg: String },
}
