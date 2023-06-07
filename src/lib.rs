use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf};

structstruck::strike! {
    #[strikethrough[derive(Debug, Serialize, Deserialize)]]
    pub struct Pup {
        pub id: String,
        #[serde(flatten)]
        pub cmd: enum Command {
            Write(pub struct WriteCmd {
                #[serde(default)]
                pub mode: pub enum WriteMode {
                    #![derive(Default)]
                    #[default]
                    Replace,
                    Append,
                },
                pub data: Vec<u8>,
                pub path: PathBuf,
            }),
            Read(pub struct ReadCmd {
                pub path: PathBuf,
                pub chunk_size: Option<u64>,
            }),
            Copy(pub struct CopyCmd {
                pub from: PathBuf,
                pub to: PathBuf,
            }),
            Exec(pub struct ExecCmd {
                #[serde(default)]
                pub env: Vec<(String, String)>,
                pub exe: PathBuf,
                #[serde(default)]
                pub argv: Vec<String>,
                pub arg0: Option<String>,
                pub wd: Option<PathBuf>,
            }),
            Listen(pub struct ListenCmd {
                pub addr: SocketAddr,
                #[serde(default)]
                pub format: enum {
                    #![derive(Default, Copy, Clone)]
                    #[default]
                    JSONL,
                    MSGPACK,
                }
            }),
            Heartbeat(pub struct HeartbeatCmd {
                pub interval_secs: f32,
            }),
        }
    }
}

impl From<WriteCmd> for Command {
    fn from(value: WriteCmd) -> Self {
        Self::Write(value)
    }
}
impl From<ReadCmd> for Command {
    fn from(value: ReadCmd) -> Self {
        Self::Read(value)
    }
}
impl From<CopyCmd> for Command {
    fn from(value: CopyCmd) -> Self {
        Self::Copy(value)
    }
}
impl From<ExecCmd> for Command {
    fn from(value: ExecCmd) -> Self {
        Self::Exec(value)
    }
}
impl From<ListenCmd> for Command {
    fn from(value: ListenCmd) -> Self {
        Self::Listen(value)
    }
}
impl From<HeartbeatCmd> for Command {
    fn from(value: HeartbeatCmd) -> Self {
        Self::Heartbeat(value)
    }
}

structstruck::strike! {
    #[strikethrough[derive(Debug, Serialize, Deserialize, Clone)]]
    pub struct Resp {
        pub id: String,
        #[serde(flatten)]
        pub res: enum Res {
            Write {},
            Copy {},
            Exec {},
            Read { path: String },
            ReadChunk { // Not broadcasted
                data: Vec<u8>,
                offset: u64,
                more: bool
            },
            Listen { addr: SocketAddr },
            Exit { code: Option<i32> },
            Output {
                out_pipe: pub enum {
                    #![derive(Copy, PartialEq, Eq)]
                    Out,
                    Err,
                },
                msg: String
            },
            Heartbeat { i: u64 },
            Error { msg: String },
        }
    }
}
