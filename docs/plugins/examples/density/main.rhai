// Phira Pro Plugin API v1; manifest.json selects compatibility version "api": 1.
fn on_chart(ctx, settings) {
    let left = 0;
    let peak = 0;
    for right in 0..ctx.notes.len() {
        while ctx.notes[right].time - ctx.notes[left].time > 1.0 {
            left += 1;
        }
        peak = max(peak, right - left + 1);
    }
    #{ algorithm: "density-example", difficulty: peak * settings.factor,
       text: "最高一秒音符数：" + peak, truncated: ctx.truncated }
}
fn on_result(ctx, settings) {
    #{ text: "本局准度：" + (ctx.accuracy * 100.0).to_string() + "%", score: ctx.score }
}
fn on_frame(ctx, settings) {
    [#{ text: "插件示例 · " + ctx.fps.to_int() + " FPS", x: 0.0, y: -0.7, color: 0x88CCFF }]
}
