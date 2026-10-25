# sherpa-onnx 模型槽位指南（拉取模型 → 适配架构）

> 目的：让人能看懂 OpenLess Windows 端本地 ASR 模型槽位的工作方式，以及如何把任意
> sherpa-onnx 格式的模型"拉取 → 适配"进 App 使用（无需改运行时代码）。

## 一、槽位机制（App 如何找到模型）

| 项 | 规则 |
|---|---|
| 目录 | `%APPDATA%/OpenLess/models/sherpa-onnx/<alias>/`（别名即槽位 id） |
| 必需文件（Qwen3Asr family） | `conv_frontend.onnx` + `encoder.int8.onnx` + `decoder.int8.onnx` + `tokenizer/`（固定文件名，App **不校验内容**） |
| 完整标记 | `.openless-model-ready`（内容 `ready\n`）；缺失则 UI 显示未下载 |
| 运行时加载 | `src-tauri/src/asr/local/sherpa_runtime.rs` → `create_offline_recognizer()`，按 family 拼固定路径 |

其他 family 的固定文件名见 `model_store.rs` 各条目（SenseVoice/Paraformer：`model.int8.onnx` +
`tokens.txt`；Whisper：`encoder.int8.onnx` + `decoder.int8.onnx` + `tokens.txt`）。

## 二、注册一个模型需要改的三处代码

1. **`crates/openless-core/src/local_asr_catalog.rs`**
   - `SHERPA_MODEL_ALIASES` 数组加别名；
   - `sherpa_family()` 的 match 里把别名映射到 family。
2. **`crates/openless-core/src/model_store.rs`**（`ModelStore::new` 里的 `add(...)` 表）
   - 有官方压缩包 → `ModelFileSelector::Archive { url, root_dir, size_bytes, sha256, required_paths }`；
   - 只有本地文件（社区转换/自建） → `ModelFileSelector::Pinned(vec![ModelFile { path, url: "", size_bytes: 精确字节数, sha256: None }, ...])`
     ——`entry_is_complete` 会核对 ready 标记 + **逐文件字节数**，换文件后必须同步改 `size_bytes`。
3. **前端选项 + 文案**
   - `src/lib/localAsr.ts`：`SherpaOnnxModelAlias` 联合类型 + `SHERPA_ONNX_ASR_MODELS` 数组；
   - `src/i18n/*.ts`：`sherpaModel<Name>` / `sherpaModel<Name>Desc` 键（8 个语言文件都要加，tsc 强校验）。

## 三、拉取社区模型并适配（三步）

1. **拉取**：拿到 sherpa-onnx 格式的模型（k2-fsa 官方 release archive，或社区 HF 转换仓库），
   解压后应能看到 `conv_frontend.onnx / encoder*.onnx / decoder*.onnx / tokenizer/` 之类的文件。
2. **适配**：把文件重命名/移动成第一节表格里的固定名，放入
   `%APPDATA%/OpenLess/models/sherpa-onnx/<你的别名>/`，然后写入标记：
   ```powershell
   Set-Content "$env:APPDATA\OpenLess\models\sherpa-onnx\<alias>\.openless-model-ready" "ready`n"
   ```
3. **注册**：按第二节改三处代码并重编译（`pnpm build` + `cargo build --release`）。
   若不想动代码，也可以直接把文件放进**已注册槽位**的目录（App 不校验内容，只认文件名）——
   但界面显示名不会变，且 Pinned 槽位会因字节数不符显示不完整。

## 四、本仓库已注册的 Qwen3 槽位

| alias | 来源 | 注册方式 |
|---|---|---|
| `qwen3-asr-0.6b-int8` | k2-fsa 官方 `sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2` | `Archive`（含 sha256） |
| `qwen3-asr-1.7b-int8` | 社区转换（本地文件，历史来源见 `_probe/official_06b_backup` 的对照组） | `Pinned`（精确字节数） |

## 五、注意事项

- **界面显示名是 i18n 写死的**，不反映目录里的真实模型；辨别真实模型看目录内文件大小或
  App 日志的 `model=` 字段。
- Qwen3-ASR 是自回归模型：超长音频（接近 30s 切块上限）+ 低信噪比有复读退化风险；
  日常长听写建议 SenseVoice（非自回归，无此问题）。
- 线程数可在「本地模型」页的「推理线程数」卡片调整（自动 = 逻辑线程 ×80%，下限 1）。
