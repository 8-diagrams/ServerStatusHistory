# ServerStatus Theme Light

<details>
<summary>界面截图</summary>

CPU 占用率图表
![screenshot 3](docs/screenshot3.png)

卡片布局
![screenshot 1](docs/screenshot1.png)

列表式布局
![screenshot 2](docs/screenshot2.png)

</details>

在线预览: [sstl-demo.orilight.top](https://sstl-demo.orilight.top/)

## 简介

使用 Vue 与 Tailwind CSS 构建的 ServerStatus 主题

基于 [ServerStatusHistory](https://github.com/8-diagrams/ServerStatusHistory) 开发

已兼容的 ServerStatus 实现（仅使用实例站点数据测试，不保证完全兼容，遇到问题请提 Issue）:

- [ServerStatusHistory](https://github.com/8-diagrams/ServerStatusHistory)
- [cppla/ServerStatus](https://github.com/cppla/ServerStatus)
- [BotoX/ServerStatus](https://github.com/BotoX/ServerStatus)

## 特性

- 卡片式/列表式布局
- 响应式界面
- CPU占用率图表
- 浏览器本地优先的通用指标历史：页面内存保存高频点、IndexedDB 保存稀疏快照，仅向服务端补拉本地缺失区间
- IndexedDB 默认 512 MiB 硬预算，超过后按最旧数据优先淘汰，不超过 1 GiB
- CPU、负载、资源使用率、网络、连接/进程、延迟和丢包等指标选择
- 5 分钟至 30 天历史范围，长时间范围自动降采样
- 不支持三网 Ping 和丢包数据的显示

历史补拉依赖 ServerStatus-Rust 的 `/api/history` 接口；实时状态仍由 `/json/stats.json` 提供。

## 使用

前往 [Release](https://github.com/orilights/ServerStatus-Theme-Light/releases) 下载 `dist.zip` 解压替换原主题即可

也可自行打包构建:

```shell
# 需要 NodeJS 环境并安装 pnpm 包管理器

# 安装依赖
pnpm i

# 打包构建
pnpm build
```

打包完成后使用 dist 目录内文件替换原主题即可

本仓库集成版本使用 `frontend/` 作为源码目录，执行 `pnpm build` 后会直接把可部署产物写入根目录的 `web/`。旗帜和系统图标复用服务端已有的 `/static/flags/4x3/` 与 `/static/os/` 资源，不需要复制上游 `public/image`。

具体操作请看 [ServerStatusHistory FAQ](https://github.com/8-diagrams/ServerStatusHistory#6-faq) 中的 **如何使用自定义主题**

## 感谢

- [ServerStatusHistory](https://github.com/8-diagrams/ServerStatusHistory)
