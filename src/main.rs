mod sync_bcast;

use futures_util::{FutureExt, StreamExt, TryStreamExt};
use log::{error, info};
use serde::Deserialize;
use std::{
    env::args,
    fs::{copy, create_dir_all, OpenOptions},
    io::{ErrorKind, Write},
    net::SocketAddr,
    process::{exit, Stdio},
};
use sync_bcast::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::TcpListener,
    sync::mpsc,
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::io::SyncIoBridge;
use well_trained_pup::*;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::init_from_env(env_logger::Env::default().filter_or("RUST_LOG", "info"));
    let caster = Caster::new(1023);
    let mut listener = caster.subscribe().await;
    tokio::spawn(exec_args(caster));
    let mut saw_error = false;
    while let Some(res) = listener.recv().await {
        saw_error = saw_error || matches!(res.res, Res::Error { .. });
        info!("Wuff: {res:?}");
    }
    info!("No more treats. Nap time!");
    exit(match saw_error {
        true => 1,
        false => 0,
    });
}

async fn exec_args(caster: Caster) {
    let args = args().collect::<Vec<_>>();
    let (backchannel, mut print) = mpsc::channel(3);
    tokio::spawn(async move {
        while let Some(print) = print.recv().await {
            info!("Bork: {print:?}");
        }
    });
    for arg in args.iter().skip(1) {
        match serde_json::from_str::<Pup>(&arg) {
            Ok(cmd) => exec_cmd(cmd, caster.clone(), backchannel.clone()).await,
            Err(err) => {
                error!("{arg:?} is not for puppies: {err}");
            }
        }
    }
}

async fn exec_cmd(cmd: Pup, results: Caster, direct_backchannel: mpsc::Sender<Resp>) {
    let res = match &cmd.cmd {
        Command::Write(data) => match write(&data) {
            Ok(()) => Ok(Res::Write {}),
            Err(e) => Err(format!("Failed to write to {}: {}", data.path.display(), e)),
        },
        Command::Read(data) => {
            let bc = |res| {
                direct_backchannel
                    .send(Resp {
                        res,
                        id: cmd.id.clone(),
                    })
                    .map(|_| ())
            };
            let path = data
                .path
                .canonicalize()
                .as_ref()
                .unwrap_or(&data.path)
                .display()
                .to_string();
            match read(&data, bc).await {
                Ok(()) => Ok(Res::Read { path }),
                Err(e) => Err(format!("Failed to read {}: {}", data.path.display(), e)),
            }
        }
        Command::Copy(CopyCmd { from, to }) => match copy(from, to) {
            Ok(_) => Ok(Res::Copy {}),
            Err(e) => Err(format!(
                "Failed to copy {} to {}: {e}",
                from.display(),
                to.display()
            )),
        },
        Command::Exec(data) => match exec(cmd.id.clone(), data, results.clone()) {
            Ok(()) => Ok(Res::Exec {}),
            Err(e) => Err(format!("failed to launch {}: {e}", data.exe.display())),
        },
        Command::Listen(data) => match listen(data, results.clone()).await {
            Ok(addr) => Ok(Res::Listen { addr }),
            Err(e) => Err(format!("Failed to listen on {}: {e}", data.addr)),
        },
    };
    let res = match res {
        Ok(res) => Resp { res, id: cmd.id },
        Err(msg) => Resp {
            id: cmd.id,
            res: Res::Error { msg },
        },
    };
    results.send(res).await;
}

async fn read<BSR: std::future::Future<Output = ()>>(
    ReadCmd { path, chunk_size }: &ReadCmd,
    direct_backchannel: impl Fn(Res) -> BSR,
) -> Result<(), std::io::Error> {
    let file = tokio::fs::File::open(path).await?;
    let file = &mut BufReader::new(file);
    let mut offset = 0;
    let chunk_size = chunk_size.unwrap_or(1 << 22);
    loop {
        let mut data = Vec::with_capacity(chunk_size as usize);
        let n = file.take(chunk_size).read_to_end(&mut data).await?;
        direct_backchannel(Res::ReadChunk {
            data,
            offset,
            more: n > 0,
        })
        .await;
        offset += n as u64;
        if n == 0 {
            break;
        }
    }
    Ok(())
}

async fn listen(
    ListenCmd { addr, format }: &ListenCmd,
    results: Caster,
) -> Result<SocketAddr, std::io::Error> {
    let server = TcpListener::bind(addr).await?;
    let addr = server.local_addr()?;
    let &format = format;
    let server = TcpListenerStream::new(server)
        .take_until(tokio::signal::ctrl_c())
        .try_for_each(move |client_conn| {
            let results = results.clone();
            async move {
                let addr = client_conn.peer_addr();
                info!("new connection from {:?}", addr);
                let (read, write) = client_conn.into_split();
                let (results_private, results_out) = results.subscribe_sidechannel().await;
                write_results(write, results_out, addr.as_ref().ok().cloned());
                read_commands(read, results, results_private, addr.ok(), format);
                Ok(())
            }
        });
    tokio::spawn(async move {
        if let Err(e) = server.await {
            error!("listener {addr} error: {e}");
        }
    });
    Ok(addr)
}

fn write_results(
    mut write: tokio::net::tcp::OwnedWriteHalf,
    mut subscribe: tokio::sync::mpsc::Receiver<Resp>,
    clone: Option<SocketAddr>,
) {
    tokio::spawn(
        async move {
            let mut write = BufWriter::new(&mut write);
            while let Some(res) = subscribe.recv().await {
                write
                    .write_all(&serde_json::to_vec(&res).expect("Res always serializable"))
                    .await?;
                write.write_all(b"\n").await?;
                write.flush().await?;
            }
            Ok::<_, std::io::Error>(())
        }
        .map(move |r| {
            if let Err(e) = r {
                log::warn!("Send to {clone:?} failed: {e}");
            }
        }),
    );
}

fn read_commands(
    read: tokio::net::tcp::OwnedReadHalf,
    results: Caster,
    results_private: mpsc::Sender<Resp>,
    addr: Option<SocketAddr>,
    format: Format,
) {
    tokio::spawn(async move {
        let read = BufReader::new(read);
        match format {
            Format::JSONL => {
                let mut read = read.lines();
                while let Some(msg) = read.next_line().await.ok().flatten() {
                    match serde_json::from_str::<Pup>(&msg) {
                        Ok(cmd) => {
                            Box::pin(exec_cmd(cmd, results.clone(), results_private.clone())).await
                        }
                        Err(err) => {
                            error!("I bite {addr:?} for {msg:?}: {err:?}");
                            break;
                        }
                    }
                }
            }
            Format::MSGPACK => {
                let handle = tokio::runtime::Handle::current();
                tokio::task::spawn_blocking(move || {
                    let read = SyncIoBridge::new_with_handle(read, handle.clone());
                    let mut read = rmp_serde::decode::Deserializer::new(read);
                    loop {
                        match Pup::deserialize(&mut read) {
                            Err(rmp_serde::decode::Error::InvalidMarkerRead(e))
                                if e.kind() == ErrorKind::UnexpectedEof =>
                            {
                                break;
                            }
                            Err(err) => {
                                error!("I bite {addr:?}: {err:?}");
                                break;
                            }
                            Ok(msg) => handle.block_on(Box::pin(exec_cmd(
                                msg,
                                results.clone(),
                                results_private.clone(),
                            ))),
                        }
                    }
                })
                .await
                .unwrap();
            }
        }
        log::info!("{addr:?} closes input");
    });
}

fn exec(
    id: String,
    ExecCmd {
        exe,
        env,
        argv,
        arg0,
        wd,
    }: &ExecCmd,
    results: Caster,
) -> Result<(), std::io::Error> {
    let cmd = &mut tokio::process::Command::new(exe);
    cmd.args(argv)
        .envs(env.iter().cloned())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if let Some(wd) = wd {
        cmd.current_dir(wd);
    }
    #[cfg(unix)]
    if let Some(arg0) = arg0 {
        cmd.arg0(arg0);
    };
    tokio::spawn(wait(id, cmd.spawn()?, results));
    Ok(())
}

async fn wait(id: String, mut child: tokio::process::Child, results: Caster) {
    let out = tokio::spawn(bcast_out(
        id.clone(),
        child.stdout.take().unwrap(),
        OutPipe::Out,
        results.clone(),
    ));
    let err = tokio::spawn(bcast_out(
        id.clone(),
        child.stderr.take().unwrap(),
        OutPipe::Err,
        results.clone(),
    ));
    let res = child.wait().await;
    out.await.ok();
    err.await.ok();
    results
        .send(Resp {
            id: id.clone(),
            res: Res::Exit {
                code: res.ok().map(|res| res.code()).flatten(),
            },
        })
        .await;
}

async fn bcast_out(id: String, stdout: impl AsyncRead + Unpin, out_pipe: OutPipe, clone: Caster) {
    let stout = BufReader::new(stdout);
    let mut stdout = stout.lines();
    while let Some(msg) = stdout.next_line().await.ok().flatten() {
        clone
            .send(Resp {
                id: id.clone(),
                res: Res::Output { out_pipe, msg },
            })
            .await;
    }
}

fn write(WriteCmd { mode, path, data }: &WriteCmd) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent).map_err(|e| format!("Failed to create parent dir: {e}"))?;
    }
    OpenOptions::new()
        .create(true)
        .write(true)
        .append(match mode {
            WriteMode::Append => true,
            WriteMode::Replace => false,
        })
        .open(path)
        .map_err(|e| format!("Failed to open file: {e}"))?
        .write_all(&data)
        .map_err(|e| format!("Failed to write data: {e}"))?;
    Ok(())
}
