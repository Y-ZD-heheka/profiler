//! CPU 密集型测试程序
//!
//! 这是一个用于测试 ETW Profiler 功能的示例程序。
//! 它模拟了多种工作负载场景，包括纯 CPU 计算、内存分配和混合操作。
//!
//! # 使用方式
//!
//! ```bash
//! # 编译测试程序
//! cargo build --example cpu_intensive
//!
//! # 运行 30 秒（默认）
//! cargo run --example cpu_intensive
//!
//! # 运行 60 秒
//! cargo run --example cpu_intensive -- 60
//!
//! # 使用 Profiler 分析（Launch 模式）
//! cargo run -- launch target/debug/examples/cpu_intensive.exe 60 -o cpu_profile.csv -d 30
//!
//! # 或先启动测试程序，再附加分析（Attach 模式）
//! # 终端1: cargo run --example cpu_intensive -- 120
//! # 终端2: cargo run -- attach <PID> -o cpu_profile.csv -d 30
//! ```

use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    // 解析命令行参数
    let args: Vec<String> = env::args().collect();
    let duration_secs = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30u64);

    println!("========================================");
    println!("  ETW Profiler CPU 密集型测试程序");
    println!("========================================");
    println!("运行时间: {} 秒", duration_secs);
    println!("工作线程: 4 个（CPU、内存、混合 x2）");
    println!("----------------------------------------");

    // 创建停止信号
    let stop_signal = Arc::new(AtomicBool::new(false));
    let progress_counter = Arc::new(Mutex::new(0u64));

    // 记录开始时间
    let start_time = Instant::now();

    // 启动 4 个工作线程
    let mut handles = vec![];

    // 线程 1: CPU 密集型任务
    let stop1 = Arc::clone(&stop_signal);
    let counter1 = Arc::clone(&progress_counter);
    handles.push(thread::spawn(move || {
        cpu_worker_thread(duration_secs, stop1, counter1, "CPU-Worker-1");
    }));

    // 线程 2: 内存分配任务
    let stop2 = Arc::clone(&stop_signal);
    let counter2 = Arc::clone(&progress_counter);
    handles.push(thread::spawn(move || {
        memory_worker_thread(duration_secs, stop2, counter2, "Memory-Worker-1");
    }));

    // 线程 3: 混合工作负载
    let stop3 = Arc::clone(&stop_signal);
    let counter3 = Arc::clone(&progress_counter);
    handles.push(thread::spawn(move || {
        mixed_worker_thread(duration_secs, stop3, counter3, "Mixed-Worker-1");
    }));

    // 线程 4: 混合工作负载
    let stop4 = Arc::clone(&stop_signal);
    let counter4 = Arc::clone(&progress_counter);
    handles.push(thread::spawn(move || {
        mixed_worker_thread(duration_secs, stop4, counter4, "Mixed-Worker-2");
    }));

    // 进度报告线程
    let stop_progress = Arc::clone(&stop_signal);
    let progress_handle = thread::spawn(move || {
        progress_reporter(duration_secs, start_time, stop_progress);
    });

    // 等待所有工作线程完成
    for handle in handles {
        handle.join().expect("工作线程 panic");
    }

    // 发送停止信号给进度线程
    stop_signal.store(true, Ordering::SeqCst);
    progress_handle.join().expect("进度线程 panic");

    let total_ops = *progress_counter.lock().unwrap();
    let elapsed = start_time.elapsed();

    println!("\n========================================");
    println!("  测试完成！");
    println!("========================================");
    println!("总运行时间: {:.2} 秒", elapsed.as_secs_f64());
    println!("总操作次数: {}", total_ops);
    println!("平均性能: {:.0} ops/秒", total_ops as f64 / elapsed.as_secs_f64());
    println!("========================================");
}

/// 进度报告线程
fn progress_reporter(duration_secs: u64, start: Instant, stop_signal: Arc<AtomicBool>) {
    let mut last_report = 0u64;

    loop {
        thread::sleep(Duration::from_secs(5));

        if stop_signal.load(Ordering::SeqCst) {
            break;
        }

        let elapsed = start.elapsed().as_secs();
        let remaining = if elapsed >= duration_secs {
            0
        } else {
            duration_secs - elapsed
        };

        // 避免重复报告
        if elapsed != last_report {
            println!("[进度] 已运行 {} 秒, 剩余 {} 秒", elapsed, remaining);
            last_report = elapsed;
        }

        if elapsed >= duration_secs {
            break;
        }
    }
}

/// CPU 工作线程入口
fn cpu_worker_thread(
    duration_secs: u64,
    stop_signal: Arc<AtomicBool>,
    counter: Arc<Mutex<u64>>,
    name: &str,
) {
    println!("[{}] 启动 CPU 密集型任务", name);

    let start = Instant::now();
    let mut local_ops = 0u64;

    while start.elapsed().as_secs() < duration_secs && !stop_signal.load(Ordering::SeqCst) {
        // 执行多层嵌套的 CPU 密集型调用
        let result = level1_cpu_task();
        local_ops = local_ops.wrapping_add(result);

        // 偶尔让出时间片
        if local_ops % 1000 == 0 {
            thread::yield_now();
        }
    }

    // 更新全局计数器
    let mut counter_guard = counter.lock().unwrap();
    *counter_guard = counter_guard.wrapping_add(local_ops);
    drop(counter_guard);
    println!("[{}] 完成, 执行了 {} 次操作", name, local_ops);
}

/// 内存工作线程入口
fn memory_worker_thread(
    duration_secs: u64,
    stop_signal: Arc<AtomicBool>,
    counter: Arc<Mutex<u64>>,
    name: &str,
) {
    println!("[{}] 启动内存分配任务", name);

    let start = Instant::now();
    let mut local_ops = 0u64;

    while start.elapsed().as_secs() < duration_secs && !stop_signal.load(Ordering::SeqCst) {
        // 执行多层嵌套的内存分配调用
        let result = level1_memory_task();
        local_ops = local_ops.wrapping_add(result);

        // 偶尔让出时间片
        if local_ops % 500 == 0 {
            thread::yield_now();
        }
    }

    let mut counter_guard = counter.lock().unwrap();
    *counter_guard = counter_guard.wrapping_add(local_ops);
    drop(counter_guard);
    println!("[{}] 完成, 执行了 {} 次操作", name, local_ops);
}

/// 混合工作线程入口
fn mixed_worker_thread(
    duration_secs: u64,
    stop_signal: Arc<AtomicBool>,
    counter: Arc<Mutex<u64>>,
    name: &str,
) {
    println!("[{}] 启动混合工作负载", name);

    let start = Instant::now();
    let mut local_ops = 0u64;

    while start.elapsed().as_secs() < duration_secs && !stop_signal.load(Ordering::SeqCst) {
        // 执行多层嵌套的混合任务调用
        let result = level1_mixed_task();
        local_ops = local_ops.wrapping_add(result);

        // 偶尔让出时间片
        if local_ops % 800 == 0 {
            thread::yield_now();
        }
    }

    let mut counter_guard = counter.lock().unwrap();
    *counter_guard = counter_guard.wrapping_add(local_ops);
    drop(counter_guard);
    println!("[{}] 完成, 执行了 {} 次操作", name, local_ops);
}

// ============================================================================
// CPU 密集型任务 - 多层嵌套调用（6 层）
// ============================================================================

/// 第 1 层: CPU 任务入口
fn level1_cpu_task() -> u64 {
    let mut sum = 0u64;
    for i in 0..10 {
        sum = sum.wrapping_add(level2_cpu_task(i));
    }
    sum
}

/// 第 2 层
fn level2_cpu_task(seed: u64) -> u64 {
    let mut sum = seed;
    for i in 0..5 {
        sum = sum.wrapping_add(level3_cpu_task(seed.wrapping_add(i)));
    }
    sum
}

/// 第 3 层
fn level3_cpu_task(seed: u64) -> u64 {
    let fib_result = fibonacci(35 + (seed % 5) as u32);
    let mat_result = matrix_multiply_task(seed);
    fib_result.wrapping_add(mat_result)
}

/// 第 4 层: 斐波那契计算
fn fibonacci(n: u32) -> u64 {
    if n <= 1 {
        return n as u64;
    }
    fibonacci(n - 1).wrapping_add(fibonacci(n - 2))
}

/// 第 4 层: 矩阵乘法任务
fn matrix_multiply_task(seed: u64) -> u64 {
    level5_matrix_calculation(seed)
}

/// 第 5 层: 矩阵计算
fn level5_matrix_calculation(seed: u64) -> u64 {
    let size = 20;
    let mut matrix_a = vec![vec![0.0f64; size]; size];
    let mut matrix_b = vec![vec![0.0f64; size]; size];
    let mut matrix_c = vec![vec![0.0f64; size]; size];

    // 初始化矩阵
    for i in 0..size {
        for j in 0..size {
            matrix_a[i][j] = ((seed + i as u64 + j as u64) % 100) as f64;
            matrix_b[i][j] = ((seed + i as u64 * j as u64) % 100) as f64;
        }
    }

    // 执行矩阵乘法
    level6_matrix_multiply(&matrix_a, &matrix_b, &mut matrix_c, size);

    // 返回校验和（使用 wrapping_add 防止溢出）
    matrix_c.iter().flatten().fold(0u64, |acc, &x| acc.wrapping_add(x as u64))
}

/// 第 6 层: 实际矩阵乘法运算
fn level6_matrix_multiply(a: &[Vec<f64>], b: &[Vec<f64>], c: &mut [Vec<f64>], size: usize) {
    for i in 0..size {
        for j in 0..size {
            let mut sum = 0.0;
            for k in 0..size {
                sum += a[i][k] * b[k][j];
            }
            c[i][j] = sum;
        }
    }
}

// ============================================================================
// 内存分配任务 - 多层嵌套调用（5 层）
// ============================================================================

/// 第 1 层: 内存任务入口
fn level1_memory_task() -> u64 {
    let mut count = 0u64;
    for i in 0..5u64 {
        count = count.wrapping_add(level2_memory_task(i.wrapping_mul(100)));
    }
    count
}

/// 第 2 层
fn level2_memory_task(size_multiplier: u64) -> u64 {
    let mut count = 0u64;
    for i in 1..=3u64 {
        count = count.wrapping_add(level3_memory_allocation(size_multiplier.wrapping_add(i.wrapping_mul(1024))));
    }
    count
}

/// 第 3 层: 内存分配
fn level3_memory_allocation(size: u64) -> u64 {
    level4_buffer_operations(size as usize)
}

/// 第 4 层: 缓冲区操作
fn level4_buffer_operations(size: usize) -> u64 {
    let mut total = 0u64;

    // 执行多种内存操作
    total = total.wrapping_add(level5_allocate_and_fill(size));
    total = total.wrapping_add(level5_allocate_and_process(size / 2));

    total
}

/// 第 5 层: 分配并填充内存
fn level5_allocate_and_fill(size: usize) -> u64 {
    let mut buffer = vec![0u8; size.min(100_000)];

    // 填充数据
    for (i, byte) in buffer.iter_mut().enumerate() {
        *byte = (i % 256) as u8;
    }

    // 计算校验和（使用 wrapping_add 防止溢出）
    buffer.iter().fold(0u64, |acc, &b| acc.wrapping_add(b as u64))
}

/// 第 5 层: 分配并处理内存
fn level5_allocate_and_process(size: usize) -> u64 {
    let actual_size = size.min(50_000);
    let mut strings: Vec<String> = Vec::with_capacity(100);

    for i in 0..100 {
        let s = format!("Data chunk {} with size {}", i, actual_size);
        strings.push(s);
    }

    strings.iter().fold(0u64, |acc, s| acc.wrapping_add(s.len() as u64))
}

// ============================================================================
// 混合任务 - 多层嵌套调用（5 层）
// ============================================================================

/// 第 1 层: 混合任务入口
fn level1_mixed_task() -> u64 {
    let mut result = 0u64;
    result = result.wrapping_add(level2_mixed_computation());
    result = result.wrapping_add(level2_mixed_allocation());
    result
}

/// 第 2 层: 混合计算
fn level2_mixed_computation() -> u64 {
    let mut sum = 0u64;

    // 执行一些 CPU 计算
    for i in 0..100 {
        sum = sum.wrapping_add(level3_heavy_math(i));
    }

    sum
}

/// 第 2 层: 混合分配
fn level2_mixed_allocation() -> u64 {
    level3_mixed_buffer_ops(500)
}

/// 第 3 层: 复杂数学运算
fn level3_heavy_math(seed: u64) -> u64 {
    let mut result = seed;

    // 级数求和
    for i in 1..=100 {
        result = result.wrapping_add(level4_series_calculation(seed, i));
    }

    // 素数检查
    result = result.wrapping_add(if level4_is_prime(seed as u32 + 1000) { 1 } else { 0 });

    result
}

/// 第 3 层: 混合缓冲区操作
fn level3_mixed_buffer_ops(count: usize) -> u64 {
    let mut buffers: Vec<Vec<u64>> = Vec::with_capacity(count);

    for i in 0..count {
        let size = 50 + (i % 100);
        let buffer: Vec<u64> = (0..size).map(|x| level4_data_transform(x as u64)).collect();
        buffers.push(buffer);
    }

    buffers.iter().fold(0u64, |acc, b| {
        acc.wrapping_add(b.iter().fold(0u64, |inner_acc, &x| inner_acc.wrapping_add(x)))
    })
}

/// 第 4 层: 级数计算
fn level4_series_calculation(seed: u64, n: u64) -> u64 {
    // 计算几何级数的一部分
    let base = (seed % 10) + 2;
    let mut result = 1u64;
    let mut term = 1u64;

    for _ in 0..n.min(10) {
        term = term.saturating_mul(base);
        result = result.saturating_add(term);
    }

    result
}

/// 第 4 层: 素数检查
fn level4_is_prime(n: u32) -> bool {
    if n <= 1 {
        return false;
    }
    if n <= 3 {
        return true;
    }
    if n % 2 == 0 || n % 3 == 0 {
        return false;
    }

    let sqrt_n = (n as f64).sqrt() as u32;
    let mut i = 5;
    while i <= sqrt_n {
        if n % i == 0 || n % (i + 2) == 0 {
            return false;
        }
        i += 6;
    }
    true
}

/// 第 4 层: 数据转换
fn level4_data_transform(x: u64) -> u64 {
    level5_complex_transform(x)
}

/// 第 5 层: 复杂转换
fn level5_complex_transform(x: u64) -> u64 {
    let mut result = x;

    // 执行多种数学运算
    result = result.wrapping_mul(2654435761); // 黄金比例常数
    result = result.wrapping_add(123456789);
    result = result.rotate_left(13);
    result ^= result >> 7;
    result = result.wrapping_mul(0x9e3779b97f4a7c15);

    result
}
