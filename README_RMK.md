# MINILA Super — RMK 0.9 配置

这是一个仅用 `keyboard.toml` 描述硬件和键位的 RMK 0.9 / RP2040 工程。`src/main.rs` 只有 RMK 配置宏，没有手写固件逻辑。

## 硬件映射

- 二极管方向：`col2row`（`row2col = false`）
- 行：ROW0–ROW4 = GPIO18–GPIO22
- 列：COL0–COL3 = GPIO8–GPIO11；COL4–COL9 = GPIO23–GPIO28；COL10–COL14 = GPIO0–GPIO4
- EC11：A = GPIO5，B = GPIO6，启用 MCU 内部上拉
- 矩阵：5 × 15，实际 70 键
- Flash 链接布局：2 MiB（适用于 2 MiB 或更大的外部 QSPI Flash）

## 配置说明

- 0 `mac_base`：默认的 Mac 主层，左侧 Alt / Gui 对调；底行两个 Fn 键进入第 1 层。
- 1 `mac_fn`：Mac 的 Fn 层。
- 2 `win_base`：Windows 主层，保留 Ctrl / Gui / Alt 的原始顺序；底行两个 Fn 键进入第 3 层。
- 3 `win_fn`：Windows 的 Fn 层。
两个 Fn 层的数字行对应 F1–F12；Fn + Q / W 输入 `(` / `)`，Fn + E / S / D / F 是上 / 左 / 下 / 右。Fn + J / K / L 是 Insert / Home / PageUp，Fn + `,` / `.` 是 End / PageDown；底行三个方向键控制静音 / 音量减 / 音量增，旋钮控制屏幕亮度。
- 4 `macro`：轻按 CapsLock 仍为大写锁定，按住进入 macro 层。在 macro 层按 `[` 选择 Windows 为默认层，按 `]` 选择 Mac 为默认层，并将选择持久化。数字键 1 / 2 输出参考配置中的两个数字宏；BackSpace 进入 BOOTLOADER 模式。
