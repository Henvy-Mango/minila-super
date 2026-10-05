# XW06A 触控条

实现基于当前锁定的 RMK 0.9.0。上滑增加音量，下滑降低音量，双击播放/暂停，单击无动作。
GPIO 按要求暂未指定：`src/main.rs` 的 `touch_pins` 为 `None`，默认固件不读取触控硬件。
算法和 RMK 接口已经接入，可先运行本机测试和轨迹回放。

## 数据路径

```text
XW06A OUT0..OUT5（低电平有效）
  → Xw06a: 每 5 ms 读取六个 GPIO
  → TouchSampleEvent: 时间戳 + 六位触摸状态
  → TouchStripProcessor / Recognizer: 去抖、滑动、双击
  → KeyboardEvent: 虚拟键按下，约 20 ms 后松开
  → RMK keymap → USB HID
```

`#[Override(entry)]` 把 input device 和 processor 加入 `run_all!`。
覆盖入口后需要显式保留全部运行任务：现有矩阵、encoder_0、存储、键盘核心、
带 host service 的 USB transport、WPM，以及新加的两个触控组件。
以后增加其他输入设备时，也要把对应任务加入这个列表。

不向 `ActionEvent` 写入按键：在此版本中它是键盘处理动作时发布的通知。
触控通过 `KeyboardEvent` 进入现有键盘处理流程，按下和释放使用可靠的异步发布。

## 虚拟键与 Vial

利用原来没有实体按键的三个矩阵位置，不改变矩阵行列数：

| 手势 | 虚拟位置 | 各层默认动作 |
| --- | --- | --- |
| 上滑 | (2,12) | AudioVolUp |
| 下滑 | (3,11) | AudioVolDown |
| 双击 | (4,4) | MediaPlayPause |

`keyboard.toml` 的 layout.map 和每层 keys 最后三项对应这些手势；Vial 中它们位于最下面单独一行，顺序相同。
这三个交点必须没有实体开关，否则实体按键也会触发同一动作。

**已有存储的键位可能覆盖新默认值。** 若刷入后手势事件正常却没有动作，
在 Vial 中把最下面三个键设成上表的动作，并检查使用的每一层。
不要为此清空整个键盘存储，以免丢失已有改键。普通按键也可以映射到这些虚拟位置。

## 无硬件测试

在项目目录执行：

```sh
rustc --edition=2021 --test src/touch_strip/gesture.rs -o /tmp/minila-touch-tests
/tmp/minila-touch-tests
rustc --edition=2021 tools/replay_touch.rs -o /tmp/minila-touch-replay
/tmp/minila-touch-replay
```

回放工具默认演示上滑、下滑、双击。也可以传入文本文件路径，或传 `-` 从标准输入读取。
每行是毫秒时间戳和六位二进制触摸状态，从 0 ms 开始，时间严格递增且为 5 ms 的倍数。
状态保持到下一行；最后继续采样 500 ms。`1` 表示触摸，右侧最低位为底部 OUT0：

```text
0    000000
100  000100
180  000000
280  000100
360  000000
```

这个例子应只输出一次 DoubleTap。回放只验证手势和虚拟键位置，不模拟 USB、RMK 调度或芯片响应。

## 手势规则

参数集中在 `src/touch_strip/gesture.rs` 的 `Config::default()`：

- GPIO 5 ms 采样，状态稳定 10 ms 后采用；这不代表 XW06A 内部的扫描周期。
- 只接受单个电极或相邻两个电极；相邻两路同时触摸时取中点。
- 相距较远的多点或三个以上电极同时触摸，会取消当前手势，直到全部释放。
- 起始位置到当前位置移动两个电极间距后，开始调音量；随后每移动一个电极间距再触发一次。
- 两次音量动作至少间隔 80 ms；一次采样最多发送一步，不对跳过的电极补发多步。
- 手指停住不重复调音量。反向滑动可以产生反向音量动作。
- 35 ms 的释放宽限容忍电极间短暂断触；过短的两次点击间隔会被合并成一次接触。
- 每次点击持续 30–250 ms，移动范围不超过一个电极间距；第二次点击须在第一次释放后 300 ms 内开始，位置相距不超过一个电极间距。
- 双击在第二次释放确认后触发；不会先发单击。滑动、长按和异常多点会取消待定双击。
- 上电等待约 500 ms，并等到未触摸状态才开始接受手势。

这些阈值是打样起点。XW06A 只提供按下/松开，没有连续电容值；识别精度和快速滑动能力
取决于芯片实际输出时序、面板和电极布局。特别要测试闲置约 80 秒后的第一次快速滑动，
因为芯片睡眠后会延长内部采样间隔。

## 以后接 GPIO

XW06A 是六路独立开漏输出，**没有 I²C 总线或独立 IRQ 引脚**。
若芯片放在触控条上，直接连接需要六路 OUT 加电源、地，至少 8 根线。
OUT0–OUT5 各自上拉到 3.3 V；RP2040 输入不能接 5 V 上拉。

选择 GPIO 后，在 entry 中将 `None` 换成：

```rust,ignore
// 这里只示意构造方式；GPIO 必须按实际接线确定。
let touch_pins = Some([
    Input::new(p.PIN_x0, Pull::Up), // 最下端电极 OUT0
    Input::new(p.PIN_x1, Pull::Up),
    Input::new(p.PIN_x2, Pull::Up),
    Input::new(p.PIN_x3, Pull::Up),
    Input::new(p.PIN_x4, Pull::Up),
    Input::new(p.PIN_x5, Pull::Up), // 最上端电极 OUT5
]);
```

需要导入 `embassy_rp::gpio::{Input, Pull}`。若方向与预期相反，反转数组的电极顺序。
实际接入后先记录六路输出，再验证快慢滑动、双击、长按、面板覆盖和睡眠唤醒。
本机测试和交叉编译不能代替这些实测。

参考：

- [RMK Input Device](https://rmk.rs/docs/features/input_device.html)
- [作者提供的 RMK 0.9 entry override 示例](https://github.com/rmk-rs/rmk/blob/rmk-v0.9.0/examples/use_config/stm32h7/src/main.rs#L85-L94)
- [芯网 XW06A 数据手册 V1.2（镜像）](https://static.chipdip.ru/lib/496/DOC013496837.pdf)
