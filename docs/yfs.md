# yfs 文件系统

yfs 是面向 noteme 的本地虚拟文件存储，用 Rust 实现。由两个物理文件组成：`header.bin`（元数据）与 `data.bin`（内容）。命名空间扁平，文件名全局唯一；`/` 仅作逻辑目录前缀，无真实目录 inode。

## 1. 目标与非目标

### 目标

- 单用户、单进程（Tauri 主进程）可靠存储笔记与附件
- 规模：最多约 65 536 个文件；单文件最大 256 MiB；库总容量最大 32 GiB
- 操作：`stat` / `list` / `read` / `write` / `delete` / `rename`
- 崩溃后可恢复到一致状态（不丢已 fsync 提交的文件）

### 非目标（v1）

- 多进程同时打开同一 yfs 库
- 多设备同步 / 远程备份
- 加密 at-rest、压缩、硬链接、符号链接、稀疏文件

## 2. 磁盘格式

字节序：little-endian。路径均相对库根目录（如 `~/.noteme/yfs/`）。

### 2.1 文件组成

```text
<store>/
  header.bin    # superblock + 固定槽位 entry 表 + freelist 头
  data.bin      # 连续 4KiB chunk 池
```

### 2.2 Superblock（`header.bin` 偏移 0，固定 4096 字节）

| 偏移 | 大小 | 字段 | 说明 |
|------|------|------|------|
| 0 | 4 | `magic` | `YFS1`（`0x31 0x53 0x46 0x59` 按 LE 存为 `u32` 常量 `0x31534659`） |
| 4 | 2 | `format_version` | 当前为 `1` |
| 6 | 2 | `flags` | bit0：需要 repair；其余保留 0 |
| 8 | 4 | `entry_size` | 固定 `512` |
| 12 | 4 | `entry_count` | 槽位总数，默认 `65536` |
| 16 | 4 | `chunk_size` | 固定 `4096` |
| 20 | 4 | `chunk_count` | `data.bin` 已分配 chunk 数（可增长） |
| 24 | 8 | `store_epoch` | 每次成功提交元数据后 +1，用于 A/B 提交选主 |
| 32 | 4 | `free_entry_head` | 空闲 entry 链表头 slot 索引，`u32::MAX` 表示空 |
| 36 | 4 | `free_chunk_head` | 空闲 extent 链表头（见 2.5），`u32::MAX` 表示空 |
| 40 | 4 | `active_side` | header 提交侧：`0`=A，`1`=B（见一致性） |
| 44 | 4 | `checksum` | 对本 superblock 前 44 字节的 CRC32C |
| 48 | 4048 | `reserved` | 填 0 |

Entry 表起始偏移：`4096`。  
Entry `i` 偏移：`4096 + i * 512`。

### 2.3 Header Entry（固定 512 字节）

| 偏移 | 大小 | 字段 | 说明 |
|------|------|------|------|
| 0 | 4 | `entry_magic` | 占用：`ENT1`；空闲槽：`FREE` |
| 4 | 2 | `flags` | bit0=tombstone（逻辑删除待 GC）；bit1=directory_hint（可选，见 API） |
| 6 | 2 | `file_type` | `0`=opaque blob，`1`=utf8 text，`2`=markdown；其余保留 |
| 8 | 8 | `size` | 逻辑字节长度 |
| 16 | 8 | `ctime_ms` | 创建时间，Unix 毫秒 UTC |
| 24 | 8 | `mtime_ms` | 修改时间 |
| 32 | 8 | `atime_ms` | 访问时间；v1 **惰性**：仅 `read`/`stat` 更新内存，不强制刷盘 |
| 40 | 32 | `content_hash` | BLAKE3-256 整文件哈希；空文件为全 0 |
| 72 | 4 | `start_chunk` | 数据起始 chunk 索引（连续 extent） |
| 76 | 4 | `n_chunks` | 占用 chunk 数；`size==0` 时为 0，`start_chunk` 忽略 |
| 80 | 4 | `next_free` | 仅 `FREE` 槽：下一空闲 slot；占用槽为 `u32::MAX` |
| 84 | 4 | `name_len` | 文件名字节数，`1..=255` |
| 88 | 256 | `name` | UTF-8 文件名，未用字节填 0；**不含**尾 `\0` 要求但填充区为 0 |
| 344 | 4 | `entry_crc` | 对本 entry 除本字段外前 344 字节的 CRC32C |
| 348 | 164 | `reserved` | 填 0 |

约束：

- 文件名唯一；比较按完整字节序列（大小写敏感）
- 禁止嵌入 `\0`；禁止前缀 `../` 段、空段（`//`）、以及名为 `.` / `..` 的路径段
- 超长（>255）**拒绝**写入，不截断
- 数据映射：**连续 extent**，`[start_chunk, start_chunk + n_chunks)`
- 4K chunk：本地分配与页对齐，与远程同步无关

### 2.4 数据文件 `data.bin`

- 由 `chunk_count` 个连续 4KiB 块组成，文件长度 = `chunk_count * 4096`
- 逻辑文件 `i` 的字节 `off` 位于：  
  `data_offset = (start_chunk * 4096) + off`，且 `off < size`
- 尾块未用字节填 0；读 API 只返回 `size` 字节
- 扩展库：将 `chunk_count` 增大并 `set_len` `data.bin`，新区域归入 free extent 链表

### 2.5 Chunk 空闲链表（嵌入 data 空闲区）

空闲空间以 **extent** 管理（连续 chunk 区间），链表头在 superblock `free_chunk_head`。

每个空闲 extent 的首 chunk 前 16 字节（小端）存放：

| 偏移 | 大小 | 字段 |
|------|------|------|
| 0 | 4 | `free_magic` = `FEXT` |
| 4 | 4 | `n_chunks` |
| 8 | 4 | `next_extent_chunk`（下一空闲 extent 起始 chunk，`u32::MAX`=尾） |
| 12 | 4 | `crc`（前 12 字节 CRC32C） |

分配：首次适配（first-fit），按需从尾部增长 `data.bin`。  
释放：插入链表并与相邻空闲 extent **合并**（地址相邻则 coalesce）。

### 2.6 内存索引

打开库时扫描全部 entry，构建：

- `HashMap<Vec<u8>, u32>`：文件名 → slot
- 校验 `entry_crc`；损坏槽记入 repair 列表

持久化只写磁盘槽位与 superblock，不单独持久化 HashMap。

## 3. 一致性、写顺序与崩溃恢复

### 3.1 Header A/B 提交

`header.bin` 逻辑布局：

```text
[superblock 4KiB]
[entry table ...]
```

提交元数据时使用 **双 superblock 尾记**：在文件末尾追加两个 4KiB 的 commit 记录区（或库初始化时预留），记为 side A / B。`active_side` 指向最新有效侧。

每次元数据事务：

1. 将脏 entry 与更新后的 freelist 字段写入 header 主体
2. `fsync(header)`
3. 写下一侧 commit 记录：`store_epoch`、`checksum`、entry 表整体 CRC32C
4. `fsync(header)`
5. 翻转 `active_side`（写在该 commit 记录内）

崩溃时选择 checksum 有效且 `store_epoch` 更大的一侧。

### 3.2 写文件（overwrite / create）顺序

Copy-on-write extent（避免原地写半截）：

1. 分配新的连续 extent，写入全部新数据 → `fsync(data)`
2. 计算 `content_hash`，准备新/更新 entry（旧 extent 记入待释放）
3. 提交 header 事务（3.1）→ 旧 extent 链入 free list
4. 若 create：占用空闲 entry 槽；若 overwrite：原地更新同一 slot（名称不变）

失败回滚：仅丢弃未提交的新 extent（下次 open 可扫描未引用 chunk 回收，见 repair）。

### 3.3 删除

1. 将 entry 标为 `FREE`，名称从内存索引移除，extent 立即链入 free list（合并）
2. 提交 header 事务

不在 v1 做延迟 tombstone；空间立即进入分配器。碎片通过 coalesce + 可选 compact 处理。

### 3.4 Rename

1. 校验新名合法且不存在
2. 更新 entry 的 `name` / `name_len` / `mtime_ms`，更新内存索引
3. 提交 header 事务（仅元数据，不搬 data）

Rename 跨逻辑「目录」与同前缀 rename 相同，均为单次原子 header 提交。

### 3.5 Compact（可选，显式触发）

当 `free_chunk` 总容量 > 已用的 25% 且空闲 extent 数 > 64 时，API `compact()` 可：

1. 按 slot 顺序把存活文件搬到新的紧凑 `data.bin`（或同文件前移）
2. `fsync(data)` 后提交新 entry 的 `start_chunk` 映射
3. 截断 `data.bin` / 重建 freelist

Compact 中进程崩溃：以 header 已提交映射为准；未引用区域由 repair 回收。

### 3.6 Repair（打开时）

若 superblock `flags.needs_repair` 或 commit checksum 失败：

1. 选定有效 commit 侧
2. 重建内存索引；收集所有被 entry 引用的 chunk 集合
3. 未被引用的 chunk 合并为空闲 extent
4. 清除 `needs_repair`，提交 header

## 4. API 语义与限制

### 4.1 路径与目录

- 存储层无目录节点；`list(prefix)` = 所有 `name` 以 `prefix` 为前缀的 entry
- 约定：若调用方把 `prefix` 当作目录，应传入带尾 `/` 的前缀（如 `notes/`）
- **不存在空目录**：删除某前缀下最后一个文件后，该「目录」自然消失
- `directory_hint` 标志预留，v1 读写忽略

### 4.2 API

| API | 语义 |
|-----|------|
| `open(path) -> Store` | 打开或创建库；加排他文件锁；建内存索引 |
| `stat(name) -> Meta` | 元数据；不存在则错误 `NotFound` |
| `list(prefix) -> Vec<Meta>` | 前缀匹配；`prefix=""` 列出全部 |
| `read(name) -> Bytes` | 整文件读取 |
| `read_at(name, off, len) -> Bytes` | 部分读；越界裁剪到文件末尾 |
| `write(name, bytes)` | 创建或整文件覆盖；COW extent |
| `delete(name)` | 删除；不存在则 `NotFound` |
| `rename(old, new)` | 改名；`new` 已存在则 `AlreadyExists` |
| `compact()` | 碎片整理 |
| `close()` | 释放锁与句柄 |

并发：进程内全局写互斥；允许多个只读重叠，与任一写互斥。不支持跨进程。

### 4.3 硬限制

| 限制 | 值 |
|------|-----|
| `format_version` | 1 |
| entry 槽位数 | 65 536（可在创建时配置，一旦写入 superblock 不可改） |
| 文件名 | 1..=255 字节 UTF-8 |
| 单文件大小 | ≤ 256 MiB |
| `data.bin` 总 chunk | ≤ 8 388 608（32 GiB） |
| 路径段 | 非空；禁止 `.`、`..`；禁止 `\0` |

### 4.4 错误类型（逻辑）

`NotFound` / `AlreadyExists` / `InvalidName` / `FileTooLarge` / `StoreFull` / `Corrupt` / `Locked` / `Io`

## 5. 实现落点（noteme）

- Rust 模块：`src-tauri/src/yfs/`（或独立 crate `yfs` 被 path 依赖）
- 经 Tauri command 暴露上述 API 的子集给 Vue 前端
- 库默认路径：应用数据目录下 `yfs/`

## 6. 操作与格式关系小结

```text
write:
  alloc extent -> write data -> fsync(data)
  -> update entry + freelist -> header A/B commit -> fsync(header)

delete / rename:
  -> update entry + freelist -> header A/B commit -> fsync(header)
```
