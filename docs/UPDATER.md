# 更新与安装

项目地址：[nightx521/wonderfulUI](https://github.com/nightx521/wonderfulUI)。
本修改版由 murainy 维护，保留 WizisCool 原作者版权及 GPL-3.0-or-later。
正式版本通过公开仓库的 GitHub Releases 分发。

## 更新方式

主界面显示后进行一次静默检查。关于页的更新按钮可手动检查，右侧小箭头选择：

- 代理加速（默认）：https://github.murainy.com/ 加完整官方 GitHub 文件地址。
- GitHub 直连：直接访问项目 GitHub Releases。

两种方式统一用于读取 latest.json 和下载安装包，选择保存到 wui:update.channel。
切换线路会失效旧检查、清除旧提示并允许重新检查；旧请求不能覆盖新状态。
下载/安装期间锁定线路。启动失败静默记录，手动失败显示简短原因并允许重试。
获取更新失败时，可切换线路后重试；签名校验失败的安装包不会安装。
手动检查加入尚未完成的启动静默检查时会共享请求；结果为最新版本也会明确提示一次。

## 后端与签名

check_project_update 只接受 proxy/github 枚举，固定项目为 nightx521/wonderfulUI。
检查超时 20 秒；下载超时 300 秒。安装包地址必须对应此项目、v<version> tag 和
WonderfulUI_<version>_x64-setup.exe 文件。后端重写选定线路后保留官方 Update 资源，
前端使用官方 SDK downloadAndInstall，下载后必须通过本修改版公钥签名校验。
版本比较沿用官方行为，不允许通过检查参数启用降级。

本机私钥保存在仓库外的用户 .tauri 目录，文件名 wonderfului-murainy.key；
公钥写入 Tauri 配置，私钥配置为该仓库的 TAURI_SIGNING_PRIVATE_KEY Actions secret。
当前私钥口令为空。维护者应妥善保管私钥，不能提交到源码或打包进客户端。
Windows 发布者证书签名是另一机制，不因生成 updater 签名而自动获得。

## 构建与发布

bun run build 需要通过环境变量提供私钥路径或内容；可选口令使用
TAURI_SIGNING_PRIVATE_KEY_PASSWORD。生产配置生成安装包与 .exe.sig。
显式 tag 发布工作流验证版本、构建签名安装器，并生成含官方地址与签名的 latest.json。
仓库改公开和创建 Release 均需用户单独明确要求，普通源码上传不触发这些动作。

## 覆盖安装

NSIS 默认覆盖升级并沿用已登记目录，保留现有资料和设置；安装和 /UPDATE
路径分别维护快传和手机访问的局域网防火墙规则，保留手机访问开关与可选的设备授权设置。
随包保留 LICENSE 与 NOTICE.md。
MSIX 开发工具代安装时，不用安装器 /R 恢复应用；等待安装器退出后通过资源管理器
启动，核对实际用户资料目录的新启动日志、导入表与账号偏好，避免继承工具的目录重定向。
分发安装包时按 GPL 向接收者提供对应源码。
