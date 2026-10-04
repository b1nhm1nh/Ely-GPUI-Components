use std::{fmt::Write as _, time::Duration};

use anyhow::{Result, bail};
use futures::future::{Either, select};
use gpui::{App, AsyncApp, WindowHandle, px};
use web_time::Instant;

use crate::{pages, shell::Gallery};

/// Scroll per frame, about a fast wheel's.
const STEP: f32 = 60.0;
const SETTLE: Duration = Duration::from_millis(500);
const LATE: Duration = Duration::from_secs(5);

/// Where the table goes: a file natively, the page on the web.
pub type Report = Box<dyn FnOnce(Result<String>, &mut App)>;

/// Times each page's switch and its scroll, frame by frame, and hands the table to `report`.
pub fn run(window: WindowHandle<Gallery>, only: Option<usize>, report: Report, cx: &mut App) {
    cx.spawn(async move |cx| {
        let table = bench_all(window, only, cx).await;
        cx.update(|cx| report(table, cx));
    })
    .detach();
}

async fn frame(window: WindowHandle<Gallery>, cx: &mut AsyncApp) -> Result<Instant> {
    let (drawn, next) = futures::channel::oneshot::channel();
    window.update(cx, |_, window, _| {
        window.on_next_frame(move |_, _| drawn.send(Instant::now()).unwrap_or_default());
        window.refresh();
    })?;
    match select(next, cx.background_executor().timer(LATE)).await {
        Either::Left((Ok(at), _)) => Ok(at),
        Either::Left((Err(_), _)) => bail!("the window dropped its frame callback"),
        Either::Right(_) => bail!("the window drew no frame in {LATE:?}; is it hidden?"),
    }
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

async fn bench_all(
    window: WindowHandle<Gallery>,
    only: Option<usize>,
    cx: &mut AsyncApp,
) -> Result<String> {
    let mut csv =
        String::from("page,switch_ms,frames,avg_ms,p95_ms,worst_ms,over_25ms,over_50ms\n");
    for (ix, page) in pages::ALL.iter().enumerate() {
        if only.is_some_and(|only| only != ix) {
            continue;
        }
        frame(window, cx).await?;
        let start = Instant::now();
        window.update(cx, |gallery, _, cx| gallery.select(ix, cx))?;
        frame(window, cx).await?;
        let switch = frame(window, cx).await? - start;
        cx.background_executor().timer(SETTLE).await;
        let (_, max) = window.update(cx, |gallery, _, _| gallery.scroll_extent())?;
        let mut times = Vec::new();
        let (mut offset, mut last) = (px(0.0), frame(window, cx).await?);
        // A page with no scroll times redraws in place.
        let still = max <= px(0.0);
        while offset < max || (still && times.len() < 60) {
            offset = (offset + px(STEP)).min(max);
            window.update(cx, |gallery, _, cx| gallery.scroll_to(offset, cx))?;
            let now = frame(window, cx).await?;
            times.push(now - last);
            last = now;
        }
        times.sort();
        let count = times.len().max(1);
        let avg = times.iter().map(|t| ms(*t)).sum::<f64>() / count as f64;
        let at = |share: f64| times.get(((times.len() as f64 * share) as usize).min(count - 1));
        let p95 = at(0.95).copied().map_or(0.0, ms);
        let worst = times.last().copied().map_or(0.0, ms);
        let over = |limit: f64| times.iter().filter(|t| ms(**t) > limit).count();
        log::info!(
            "bench: {:<14} switch {:>7.1} ms, {:>4} frames, avg {:>5.1}, p95 {:>5.1}, worst {:>6.1}, >25 {:>3}, >50 {:>3}",
            page.slug,
            ms(switch),
            times.len(),
            avg,
            p95,
            worst,
            over(25.0),
            over(50.0)
        );
        writeln!(
            csv,
            "{},{:.1},{},{avg:.1},{p95:.1},{worst:.1},{},{}",
            page.slug,
            ms(switch),
            times.len(),
            over(25.0),
            over(50.0)
        )?;
    }
    Ok(csv)
}
