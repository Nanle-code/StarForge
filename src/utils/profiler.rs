use std::time::{Duration, Instant};

#[cfg(feature = "memory-profiling")]
use std::alloc::{GlobalAlloc, Layout, System};
#[cfg(feature = "memory-profiling")]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "memory-profiling")]
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "memory-profiling")]
static DEALLOCATED: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "memory-profiling")]
static CURRENT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "memory-profiling")]
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Allocation-counting global allocator.
///
/// Register it from the binary with `#[global_allocator]` to make
/// [`Profiler::get_memory_metrics`] report real numbers.
///
/// Only atomic counters are updated here, deliberately: allocating a
/// collection inside `alloc` would re-enter the allocator and recurse.
#[cfg(feature = "memory-profiling")]
#[derive(Debug)]
pub struct MemoryProfiler;

#[cfg(feature = "memory-profiling")]
unsafe impl GlobalAlloc for MemoryProfiler {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        // Note: ALLOC_TRACKER is from origin/master
        #[cfg(feature = "memory-profiling")]
        {
            ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
            CURRENT.fetch_add(layout.size(), Ordering::Relaxed);
            let current = CURRENT.load(Ordering::Relaxed);
            let mut peak = PEAK.load(Ordering::Relaxed);
            while current > peak {
                match PEAK.compare_exchange_weak(
                    peak,
                    current,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(p) => peak = p,
                }
            }
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        #[cfg(feature = "memory-profiling")]
        {
            DEALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
            CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
        }
    }
}

/// Snapshot of the global allocation counters.
#[cfg(feature = "memory-profiling")]
fn allocator_snapshot() -> (usize, usize, usize, usize) {
    (
        ALLOCATED.load(Ordering::Relaxed),
        DEALLOCATED.load(Ordering::Relaxed),
        CURRENT.load(Ordering::Relaxed),
        PEAK.load(Ordering::Relaxed),
    )
}

pub struct Timer {
    start: Instant,
}

impl Timer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

#[derive(Debug, Clone)]
pub struct ProfilePoint {
    pub label: String,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct MemoryPoint {
    pub label: String,
    pub timestamp: Duration,
    pub allocated_bytes: usize,
    pub deallocated_bytes: usize,
    pub current_bytes: usize,
    pub peak_bytes: usize,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryMetrics {
    pub allocated: usize,
    pub deallocated: usize,
    pub current: usize,
    pub peak: usize,
    pub samples: Vec<MemoryPoint>,
}

#[derive(Debug)]
pub struct Profiler {
    start: Instant,
    marks: Vec<(String, Instant)>,
    #[cfg(feature = "memory-profiling")]
    memory_tracker: Option<MemoryTracker>,
}

#[cfg(feature = "memory-profiling")]
#[derive(Debug)]
struct MemoryTracker {
    // Not currently called from any code path in this crate. Kept rather than
    // removed since deleting it is a product decision, not a lint-scoping one.
    #[allow(dead_code)]
    start: Instant,
    current_memory: usize,
    peak_memory: usize,
    samples: Vec<(String, Duration, usize, usize, usize, usize)>,
}

#[cfg(feature = "memory-profiling")]
impl MemoryTracker {
    /// Records the allocator counters as they stand at `elapsed`.
    fn record_sample(&mut self, label: String, elapsed: Duration) {
        let (allocated, deallocated, current, peak) = allocator_snapshot();
        self.current_memory = current;
        self.peak_memory = peak;
        self.samples
            .push((label, elapsed, allocated, deallocated, current, peak));
    }
}

impl Profiler {
    pub fn start() -> Self {
        #[cfg(feature = "memory-profiling")]
        let memory_tracker: Option<MemoryTracker> = Some(MemoryTracker {
            start: Instant::now(),
            current_memory: 0,
            peak_memory: 0,
            samples: Vec::new(),
        });

        Self {
            start: Instant::now(),
            marks: Vec::new(),
            #[cfg(feature = "memory-profiling")]
            memory_tracker,
        }
    }

    pub fn mark(&mut self, label: impl Into<String>) {
        let label_str = label.into();
        let at = Instant::now();

        #[cfg(feature = "memory-profiling")]
        {
            let label_for_tracker = label_str.clone();
            let elapsed = at.duration_since(self.start);
            if let Some(tracker) = &mut self.memory_tracker {
                tracker.record_sample(label_for_tracker, elapsed);
            }
        }

        self.marks.push((label_str, at));
    }

    pub fn get_memory_metrics(&self) -> MemoryMetrics {
        #[cfg(feature = "memory-profiling")]
        if let Some(tracker) = &self.memory_tracker {
            let (allocated, deallocated, current, peak) = allocator_snapshot();
            return MemoryMetrics {
                allocated,
                deallocated,
                current,
                peak,
                samples: tracker
                    .samples
                    .iter()
                    .map(|(label, timestamp, a, d, c, p)| MemoryPoint {
                        label: label.clone(),
                        timestamp: *timestamp,
                        allocated_bytes: *a,
                        deallocated_bytes: *d,
                        current_bytes: *c,
                        peak_bytes: *p,
                    })
                    .collect(),
            };
        }

        let mut metrics = MemoryMetrics::default();
        for (label, at) in &self.marks {
            metrics.samples.push(MemoryPoint {
                label: label.clone(),
                timestamp: at.duration_since(self.start),
                allocated_bytes: 0,
                deallocated_bytes: 0,
                current_bytes: 0,
                peak_bytes: 0,
            });
        }
        metrics
    }

    pub fn points(&self) -> Vec<ProfilePoint> {
        let mut last = self.start;
        let mut points = Vec::with_capacity(self.marks.len());
        for (label, at) in &self.marks {
            points.push(ProfilePoint {
                label: label.clone(),
                elapsed: at.duration_since(last),
            });
            last = *at;
        }
        points
    }

    pub fn total_elapsed(&self) -> Duration {
        match self.marks.last() {
            Some((_, at)) => at.duration_since(self.start),
            None => Duration::from_millis(0),
        }
    }
}
