# 首页目录、标签与按需加载改造方案

## 目标

首页需要同时支持两种语义不同、但可以组合的主机管理维度：

1. **目录**：按服务端管理的 `gid` 归属查看。一台主机只能属于一个目录。
2. **标签**：按 `tags` 做横向筛选。一台主机可以拥有多个标签；多个已选标签使用 OR。

二者的组合语义统一为：`目录 AND（标签 1 OR 标签 2 OR ...）`。标签是目录下方的附加筛选条件，不再与目录并列为独立浏览模式。

首页默认进入 `default` 目录（界面显示“默认”），即没有目录信息的主机集合，避免首次打开就加载全部主机。

## 数据语义

### 目录

- `server.gid` 非空：进入对应目录。
- `server.gid` 为空：进入虚拟目录 `default`。
- `default` 不是配置文件中的特殊主机组，不需要 Agent 发送新字段。
- `default` 和 `all` 是保留目录 ID，配置中的 `gid` 不应使用这两个值。
- 固定主机可以在 `hosts` 中配置 `gid`；动态主机继续使用 `hosts_group[].gid`。
- 目录是唯一归属关系，查询结果不能因为命中多个标签而复制主机。

### 标签

- `server.tags` 是服务端管理的多值属性，Agent 不能覆盖。
- 标签筛选支持多选，语义为 OR：命中任意一个已选标签即可展示。
- 同一主机可以被多个标签命中，但一次 API 查询只返回该主机一次。
- 主机历史缓存仍以 `host + metric` 为键，不能因标签重复保存历史数据。

## 后端 API

保留现有接口，确保旧 Web 和旧 Agent 不受影响：

- `/report`：不修改 HTTP 上报协议。
- gRPC protobuf：不修改。
- `/json/stats.json`：继续返回完整实时列表，兼容旧主题。

新增接口：

### `GET /api/hosts/catalog`

返回左侧菜单所需的目录和标签概要，不返回完整指标：

```json
{
  "default": {"name": "默认", "count": 128},
  "directories": [
    {"id": "FINTECH", "name": "FINTECH", "count": 32}
  ],
  "tags": [
    {"name": "AWS", "count": 56}
  ]
}
```

### `GET /api/hosts`

按目录和标签组合查询当前实时主机列表，响应结构复用：

```json
{"updated": 1730000000, "servers": []}
```

查询约定：

```text
/api/hosts?directory=default
/api/hosts?directory=FINTECH
/api/hosts?tag=AWS&tag=MYSQL
/api/hosts?directory=all&tag=AWS&tag=MYSQL
/api/hosts?directory=FINTECH&tag=AWS&tag=MYSQL
/api/hosts?directory=default&tag=AWS&tag=MYSQL
```

规则如下：

- 多个 `tag` 参数表示 OR，不表示 AND。
- 目录与标签组合时使用 AND。
- `directory=all`：不限制目录。
- `directory=default`：只匹配 `gid` 为空的主机。
- `directory=<gid>`：只匹配该目录。
- 不传目录但传标签：在全部目录中执行标签 OR 筛选。
- 不传标签：只应用目录条件。
- 目录和标签匹配均不区分大小写。
- 无任何参数：仍只返回 `default`，避免误触发全量首页加载。

## 前端行为

1. 首次打开先加载 catalog，再请求 `directory=default`。
2. 左侧菜单先显示目录列表，包括“全部机器”“默认”和各个 `gid`。
3. 标签多选固定放在目录列表下方，是当前目录的附加筛选条件。
4. 标签不选时展示当前目录全部主机；多选时展示命中任一标签的主机。
5. 查询结果按主机展示一次，不按标签复制段落。
6. 切换目录或标签时只请求对应实时数据，当前页面之外的主机不参与首屏渲染和历史补拉。
7. 当前小时和当天优先使用浏览器 IndexedDB / 内存实时数据；缺口再访问 `/api/history`。
8. IndexedDB 仍受 1 GiB 上限约束。
9. 浏览器刷新丢失内存数据不影响服务端历史查询。

## 布局模式

布局模式是展示层设置，不改变目录和标签语义。首页顶部保留三种模式：

- **网格**：固定列数的卡片网格，适合快速浏览大量主机。
- **自适应**：卡片按可用宽度自动换行，适合不同屏幕尺寸。
- **列表**：单列详细列表，适合查看主机状态细节。

布局模式与目录/标签筛选独立：可以在默认目录使用网格，也可以在标签多选结果使用列表；切换布局不应重新请求服务端数据。

## 兼容与演进

- 新版前端优先使用 catalog + 按需主机 API。
- 旧版前端继续轮询 `/json/stats.json`。
- 若后端暂时不支持新 API，前端保留 `/json/stats.json` 降级路径，并在浏览器内按 `gid` / `tags` 过滤。
- 后续可将 `/api/hosts` 的查询实现抽象到数据源层，兼容 SQLite、VictoriaMetrics、InfluxDB 等历史/实时数据源。

## 实施顺序

1. 后端增加 catalog 生成和目录/标签过滤接口。
2. 为 `default`、目录唯一归属、标签 OR 查询增加服务端测试。
3. 前端增加 catalog 状态和按需加载状态。
4. 将当前左侧菜单改为默认目录 + 多标签选择。
5. 接入布局模式和旧接口降级。
6. 构建前端并运行 Rust / TypeScript 验证。
