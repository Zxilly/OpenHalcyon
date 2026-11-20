# 4.13.4 服务端补充数据

在实例配置的顶层指定基础 delivery 和补充包。相对路径从配置文件所在目录解析。

```toml
data_dir = "data/delivery"
data_extra_dir = "data/server_data_extra"
```

服务端和管理面板共享进程内的 GameData。房间按需使用已解析的地面网格。匹配池用对局数据摘要和模拟器源码摘要检查规则兼容性。语言词典、协议统计和会话参考表有独立的参考资料摘要。运营者可以在管理面板的 **Game data** 中选择目录、搜索和翻页，或通过已登录的管理会话访问 `GET /api/admin/v1/content`。原始数据留在实例的配置目录。[管理 API](admin-api.v1.yaml)

| 来源 | 已接入的行为和可核对的数量 |
| --- | --- |
| `abilities.json` | 按 61 个英雄索引 277 条技能、1,599 个技能变量、19 个英雄额外变量和 180 个伤害块。查询支持等级、倍率和按 tooltip 标识哈希选择伤害块。Ringo 的普通技能和大招使用不同的命中、灼烧、攻速与耗能规则，Idris 的飞轮分别计算去程和回程，Lorelai 的水池和护盾使用各自的表。 |
| `units_full.json` | 219 个完整单位定义供查询。模拟器在已绑定模板中读取成长、回复、攻速、视野及奖励。Solo 远程、近战、攻城、队长兵的基础经验依次为 6、8、9、11，基础金币为 25、35、60、100。ARAL 三类兵在双向记录中的单人补刀奖励吻合双倍基础值。 |
| `talents_full.json` | 189 条天赋、436 个修饰项，319 项随等级变化。171 条有公开 ID 和所属英雄，可生成原生天赋目录，进度按本服账号投影。具名 Ringo Long Shot 在模拟命令中按等级修改射程、伤害和冷却。把 ARAL 数字选项映射到具名天赋仍需对应证据。 |
| `items_full.json`、`item_stores.json` | 93 件装备和五份有序商店表。已实现的装备从模式对应商店检查上架资格，按组件实例结算合成、出售和属性撤销。四份出售回执核对了退款列。Bot 能购买并激活 Sprint Boots；ARAL Healing Flask 使用持有槽、屏障、恢复和冷却。其他装备的主动或被动还需各自的触发规则。[装备说明](equipment-catalog.md) |
| `heroes_and_skins.json` | 290 条官方皮肤、192 个主题、304 条 CFF0 皮肤条目和六个大厅清单。选皮检查哈希、实际英雄归属和实例公告范围。57 位公告英雄上的 218 个皮肤通过归属核对。启动时生成客户端所需的字符串化皮肤目录，显式配置的归档优先。 |
| `maps/navmesh_index.json` 和 16 个网格 | 8,059 个 Detour tile。地图查询保留源 OBB 标识和本地二进制 SHA-256。ARAL 和 Solo 英雄移动按地面连通性投影目标、跨 tile 寻路，Bot 追击和撤退使用同一入口。每个房间每 tick 最多进行六次寻路，缓存每名英雄的当前路径。[地图说明](navmesh.md) |
| `maps/level_layouts.json`、`maps/camps_and_modes.json` | 25 个关卡和 139 条营地或模式定义。已识别的塔、出生台、基地商店和草丛分别进入对应的运行逻辑。营地摆放与实体定义按 ID 连接，可计算已确认变换的世界坐标。移动网格、草丛和视野网格有不同语义。 |
| `cff0_complete_index.json` | 按资源哈希校验并索引 942 项，可查包内相对路径、资源名称、大小和补丁计数。管理 API 同时支持用名称和哈希搜索。 |
| `localization/` 下的三份文件 | 14 个语言索引项，实包提供英语 7,329 个键和简体中文 7,328 个键。查询返回实际命中的语言，缺失的 `RETRIEVE_CREDENTIALS_DESCRIPTION` 自动回退到英语。 |
| `server_constants.json` | 属性字典、单位及合成公式可从管理 API 查询。原生会话读取功能标记、15 组实验和超时阈值。聊天、状态订阅、连接授权及路由反映当前实例配置。 |
| `protocol_empirical/` 的两份文件 | 105 条 opcode 汇总保留方向、长度分布和 reader 对照；十条英雄快照验证实体与皮肤哈希。三场 ARAL 的 176,197 条含六条明文握手，Solo 的 43,160 条来自已有 VGR。管理 API 只输出聚合字段。 |

## 选人与对局行为

缺少 `catalog.json` 和选人记录时，实例可从官方可见的默认皮肤与英雄实体的对应关系生成选人列表。对这个补充包生成的 57 位英雄，测试逐一与原生选人公告的英雄集合核对。额外可选皮肤由实例提供的公告目录决定。

Solo 已接入 12 个营地在首轮生成的 18 只普通野怪，双方均可攻击，野怪有独立身份、血量、死亡与本服奖励。10 次记录内的击杀覆盖五个模板。野怪追击、再次刷新和两个龙坑的行为仍需不同证据。[Solo 营地](solo-camps.md)

工具提示中的带参标识可以和本技能的伤害块按哈希核对。已有 154 个块获得无歧义的标识，其余 26 个保留原始头字段。管理面板的技能记录提供所选语言的说明。Reim 的 Winter Spire 分两次检查范围内的目标，其释放广播、技能等级与取样范围列在[技能表](native-abilities.md)中。完整消息与实录逐项对照的命令见[协议数据校验](empirical-verification.md)。

## 本地验证

```powershell
$env:HALCYON_DELIVERY = (Resolve-Path ../vg/_ana/delivery).Path
$env:HALCYON_DATA_EXTRA = (Resolve-Path ../vg/server_data_extra).Path
cargo test --locked -p halcyon-data --lib -- --include-ignored
cargo test --locked -p halcyon-sim --lib -- --include-ignored
cargo test --locked -p halcyon-admin --test operations content_catalog -- --ignored
cargo test --locked -p halcyon-ce-platform --test android supplement_serves -- --ignored
```

最后一项从 delivery 的上一级读取两个归档回包，逐字段比对公开皮肤目录与天赋标识，并检查本服的会话路由。包中的等级上限估值、未命名数值槽和天赋费用样例可用于研究，不能单独确定原生按键、触发时刻或玩家的已购等级。各协议的采样方向、命令处理和广播状态列在[协议覆盖表](protocol-coverage.md)与[服务端输出表](server-emissions.md)中。
