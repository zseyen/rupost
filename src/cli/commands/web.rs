//! `rupost web`（别名 w）：活文档工作台服务参数。

use clap::Parser;

#[derive(Parser, Debug)]
pub struct WebArgs {
    /// 监听端口
    #[arg(short, long, default_value_t = 3000)]
    pub port: u16,

    /// 监听地址
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// 自定义外部前端静态资源目录（覆盖内置前端）
    #[arg(long, value_name = "DIR")]
    pub static_dir: Option<String>,

    /// 开启只读保护模式（禁止保存与覆盖文件）
    #[arg(long)]
    pub readonly: bool,

    /// 追加允许访问工作台的 Host 白名单（可逗号分隔多项，如内网域名），防御 DNS Rebinding
    #[arg(long, value_name = "HOST", value_delimiter = ',')]
    pub allowed_hosts: Vec<String>,

    /// 开发者跨域模式：挂载宽松 CORS 供独立前端（Vite/Webpack）跨域联调。
    /// 仅允许绑定本地回环地址（127.0.0.1/localhost/::1），非回环 --host 直接拒绝
    #[arg(long)]
    pub dev: bool,

    /// 活文档根目录
    #[arg(short = 'd', long = "dir", default_value = ".")]
    pub dir: String,
}
