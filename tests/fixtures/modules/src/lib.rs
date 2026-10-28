mod a;
mod c;
mod linked;
#[cfg_attr(unix, path = "imp/unix.rs")]
#[cfg_attr(windows, path = "imp/windows.rs")]
mod imp;
#[path = "elsewhere/p.rs"]
mod p;

mod inline {
    mod e;
}
