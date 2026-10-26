# tauri-runtime-mb

基于 Miniblink mb132 的 Tauri 3 自定义运行时，使用 `mb132_x64.dll` 渲染页面，适用于 Windows x64 桌面应用。

项目使用 Rust 实现原生窗口、Win32 消息循环、输入转发，以及 Tauri 自定义协议和 IPC 的适配。浏览器引擎通过 DLL 动态加载，不需要 Miniblink 的导入库，也不依赖 WebView2 进行页面渲染。

目前处于开发阶段，支持基本应用运行，但尚未完整实现 Tauri Runtime 接口。接入前请阅读下方的[已知限制](#已知限制)。

## 项目结构

```text
crates/
  mb132-sys/          mb132 的 FFI 声明与 DLL 动态加载
    vendor/          mb132_x64.dll
  mb132/              引擎初始化、WebView、脚本执行与网络拦截封装
  tauri-runtime-mb/   Tauri Runtime、原生窗口、输入与 IPC 适配
examples/
  miniblink-app/      可直接运行的 Tauri 示例
    dist/            静态 HTML 页面，无需前端构建工具
    src/main.rs      greet / engine 示例命令
```

## 当前功能

- 加载页面、执行 JavaScript、注入初始化脚本。
- 通过自定义协议提供应用资源，通过 Tauri `invoke` 调用 Rust 命令。
- 创建 Win32 窗口，处理基本窗口事件、焦点、鼠标和键盘输入。
- 无边框窗口的原生边框计算、阴影开关和边缘缩放；最大化时按显示器工作区布局。
- 中文 IME 内联预编辑，以及系统候选窗口定位。
- 配置引擎 DLL 路径、数据目录、User-Agent 等参数。

Tauri 运行时当前采用离屏渲染：Miniblink 生成绘制表面，宿主将内容绘制到原生窗口，并向引擎转发输入事件。

## 环境要求

- Windows 10 / 11，x64。当前绑定不支持 Windows x86、ARM64、Linux 或 macOS。
- Rust 1.95 或更新版本，使用 `x86_64-pc-windows-msvc` 工具链。
- Visual Studio Build Tools 的 C++ 桌面开发组件和 Windows SDK。
- 引擎文件 `crates/mb132-sys/vendor/mb132_x64.dll`。

项目使用 Rust 2024 edition 和 Tauri 3 alpha API。请保留仓库的 `Cargo.lock`；Tauri 2 的接入示例不能直接照搬。

## 快速运行

在仓库根目录的 PowerShell 中执行：

```powershell
cargo run -p miniblink-app --locked --target-dir target
```

示例前端是 `examples/miniblink-app/dist/index.html`，不需要安装 Node.js、npm 或启动前端开发服务器。打开后点击 **Invoke**，可以调用 Rust 的 `greet` 和 `engine` 命令，查看返回文本与引擎信息。

示例当前配置为无边框窗口。若需要系统标题栏，可将 `examples/miniblink-app/tauri.conf.json` 中的 `app.windows[0].decorations` 改为 `true`。没有自定义关闭按钮时，可以使用 `Alt+F4` 关闭窗口。

构建发布版本：

```powershell
cargo build -p miniblink-app --release --locked --target-dir target
```

输出文件位于 `target/release/`。示例的 `build.rs` 会尝试将引擎 DLL 复制到该目录，发布前应确认 DLL 已存在。当前示例设置了 `bundle.active = false`，上述命令只构建程序，不生成安装包。

## 接入 Tauri 应用

可先参考 [示例 Cargo.toml](examples/miniblink-app/Cargo.toml)、[构建脚本](examples/miniblink-app/build.rs) 和 [应用入口](examples/miniblink-app/src/main.rs)。

`tauri-runtime-mb` 已发布到 crates.io，在应用的 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
tauri = "3.0.0-alpha.4"
tauri-runtime-mb = "0.0.1"

[build-dependencies]
tauri-build = "3.0.0-alpha.4"
```

也可以改用本地源码依赖，把 `tauri-runtime-mb = "0.0.1"` 换成 `{ path = "../tauri-runtime-mb/crates/tauri-runtime-mb" }`。

应用需要正常的 Tauri 配置、前端资源和 `build.rs`：

```rust
fn main() {
    tauri_build::build();
}
```

在应用入口显式指定 Runtime 类型：

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

type Runtime = tauri_runtime_mb::MbRuntime<tauri::EventLoopMessage>;

fn main() {
    tauri::Builder::<Runtime>::new()
        .runtime(tauri_runtime_mb::MbInitAttrs::default())
        .run(tauri::generate_context!())
        .expect("failed to run the miniblink runtime");
}
```

引擎 DLL 需要自行部署；上面的最小 `build.rs` 只执行 Tauri 构建步骤，不会复制 DLL。也可复用仓库示例的复制逻辑。应用清单应保留 Common Controls v6 和 DPI awareness 设置，参见 [app.manifest](examples/miniblink-app/app.manifest)。

## 引擎配置与部署

`MbInitAttrs` 提供以下选项：

| 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `dll_path` | `None` | 指定 `mb132_x64.dll` 的路径；为空时使用默认查找顺序 |
| `nodejs` | `false` | 向引擎请求启用 Node.js 集成；能力取决于所用引擎构建 |
| `config` | `None` | 传给 Miniblink `mbSettings::config` 的配置字符串 |

例如，将入口中的 `.runtime(...)` 替换为：

```rust
.runtime(tauri_runtime_mb::MbInitAttrs {
    dll_path: Some(std::path::PathBuf::from(r"D:\my-app\mb132_x64.dll")),
    ..Default::default()
})
```

默认按以下顺序查找 DLL：

1. 环境变量 `MB132_DLL_DIR` 指定的目录（值为空时忽略）。
2. 当前可执行文件所在目录。
3. 当前工作目录。
4. 若上述路径均不存在，将 DLL 名称交给系统加载器处理。

`MB132_DLL_DIR` 在运行时读取，适合按机器或部署位置调整引擎路径，无需重新编译：

```powershell
$env:MB132_DLL_DIR = "D:\my-app\engine"
```

发布时建议保持以下目录结构，不依赖开发机的源码路径：

```text
my-app/
  miniblink-app.exe
  mb132_x64.dll
```

## 开发与验证

以下命令均在仓库根目录运行：

```powershell
# 检查整个 workspace
cargo check --workspace --locked --target-dir target

# 静态检查
cargo clippy --workspace --all-targets --locked --target-dir target

# 库测试，包括默认忽略的原生引擎回归
cargo test --workspace --lib --locked --target-dir target -- --include-ignored --test-threads=1
```

原生 IME 测试会加载实际的 mb132 DLL，并运行隐藏窗口的 Win32 消息循环；窗口边框测试直接检查原生窗口尺寸与命中结果。请在 Windows 桌面环境中执行。依赖已缓存时，可为 Cargo 命令添加 `--offline`。

完整的 `cargo test --workspace` 还会执行文档测试。目前 `miniblink` 和 `tauri-runtime-mb` 的 crate 文档示例存在编译问题，不能将库测试通过视为完整测试已通过。

启用运行日志：

```powershell
$env:MB_TRACE = "1"
cargo run -p miniblink-app --locked --target-dir target
Remove-Item Env:MB_TRACE
```

日志包含窗口消息、协议请求和页面控制台等信息，适合排查资源加载、IPC 与输入问题。

## 已知限制

当前实现尚不能作为 Wry Runtime 的完整替代，主要限制包括：

- **HTML 加载**：直接使用 `mb132::WebViewBuilder::html` 或 `WebView::load_html` 时应提供有效的 base URL。传入 `None` 在随附引擎上已复现访问冲突。
- **线程调度**：部分窗口和 WebView 操作仍直接调用原生 API。例如从工作线程调用窗口 `destroy()`，可能返回成功但窗口未被销毁。此类操作应通过 `run_on_main_thread` 调度。
- **退出流程**：关闭最后一个窗口并阻止退出后，后续显式退出请求可能被忽略。托盘常驻等场景需要额外验证。
- **多 WebView**：同一窗口中的子 WebView 输入路由尚不完整，当前优先使用单窗口单 WebView。
- **Cookie 与数据清理**：主线程同步读取 Cookie 会超时；`cookies_for_url` 尚未按 URL 过滤，`clear_all_browsing_data` 当前只清理 Cookie。
- **全屏**：初始全屏与退出全屏后的窗口状态恢复尚未完善。
- **接口覆盖**：部分接口仍为空实现，例如 WebView `reparent`、背景色设置，以及部分窗口效果和光标操作；返回 `Ok(())` 不一定代表已经生效。
- **外观验证**：无边框逻辑参考 Wry/Tao 的 Windows 实现，已有原生回归覆盖，但仍需在不同 Windows 版本、主题与显示器缩放设置下检查实际外观。

