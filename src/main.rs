mod sync_bcast;

use futures_util::{FutureExt, StreamExt, TryStreamExt};
use log::{error, info};
use std::{
    env::args,
    fs::{copy, create_dir_all, OpenOptions},
    io::Write,
    net::SocketAddr,
    process::{exit, Stdio},
};
use sync_bcast::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader, BufWriter},
    net::TcpListener,
};
use tokio_stream::wrappers::TcpListenerStream;
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
    for arg in args.iter().skip(1) {
        match serde_json::from_str::<Pup>(&arg) {
            Ok(cmd) => exec_cmd(cmd, caster.clone()).await,
            Err(err) => {
                error!("{arg:?} is not for puppies: {err}");
            }
        }
    }
}

async fn exec_cmd(cmd: Pup, results: Caster) {
    let res = match &cmd.cmd {
        Command::Write(data) => match write(&data) {
            Ok(()) => Ok(Res::Write {}),
            Err(e) => Err(format!("Failed to write to {}: {}", data.path.display(), e)),
        },
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

async fn listen(
    ListenCmd { addr }: &ListenCmd,
    results: Caster,
) -> Result<SocketAddr, std::io::Error> {
    let server = TcpListener::bind(addr).await?;
    let addr = server.local_addr()?;
    let server = TcpListenerStream::new(server)
        .take_until(tokio::signal::ctrl_c())
        .try_for_each(move |client_conn| {
            let results = results.clone();
            async move {
                let addr = client_conn.peer_addr();
                info!("new connection from {:?}", addr);
                let (read, write) = client_conn.into_split();
                write_results(
                    write,
                    results.subscribe().await,
                    addr.as_ref().ok().cloned(),
                );
                read_commands(read, results, addr.ok());
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

fn read_commands(read: tokio::net::tcp::OwnedReadHalf, results: Caster, addr: Option<SocketAddr>) {
    tokio::spawn(async move {
        let stout = BufReader::new(read);
        let mut stdout = stout.lines();
        while let Some(msg) = stdout.next_line().await.ok().flatten() {
            match serde_json::from_str::<Pup>(&msg) {
                Ok(cmd) => Box::pin(exec_cmd(cmd, results.clone())).await,
                Err(err) => {
                    error!("I bite {addr:?} for {msg:?}: {err}");
                    break;
                }
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
