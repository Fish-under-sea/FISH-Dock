//! 自有插件进程树随实例回收；不根据持久化 PID 结束外部程序。
#[cfg(windows)]
mod platform {
    use std::process::Child;
    pub struct Owner(Option<crate::scripts::job::Job>);
    impl Owner {
        pub fn attach(child:&Child)->Result<Self,String>{
            let job=crate::scripts::job::Job::new().ok_or("插件进程组创建失败")?;
            if !job.assign(child){job.close();return Err("插件进程组绑定失败".into());}
            Ok(Self(Some(job)))
        }
    }
    impl Drop for Owner {fn drop(&mut self){if let Some(job)=self.0.take(){job.close();}}}
}
#[cfg(not(windows))]
mod platform {
    pub struct Owner;
    impl Owner {pub fn attach(_child:&std::process::Child)->Result<Self,String>{Err("当前插件进程树监管仅支持 Windows".into())}}
}
pub use platform::Owner;
