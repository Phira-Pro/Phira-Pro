fn tree_value(tree, x) {
    let node = tree;
    while !node.contains("leaf") {
        let index = node["feature"];
        node = if x[index] <= node["threshold"] { node["left"] } else { node["right"] };
    }
    node["leaf"]
}

fn model_value(model, x) {
    let learner = model["learner"];
    let value = learner["base"];
    for tree in learner["trees"] {
        value += learner["lr"] * tree_value(tree, x);
    }
    let deshrink = model["deshrink"];
    deshrink["slope"] * value + deshrink["intercept"]
}

fn shape_value(u, tail) {
    if u <= 0.0 { return 0.0; }
    let shape = tail["shape"];
    if shape == "linear" { return u; }
    if shape == "exp" {
        let k = tail["expK"];
        if k <= 0.0 { return u; }
        let ku = k * u;
        return if ku > 40.0 { exp(40.0) / k } else { (exp(ku) - 1.0) / k };
    }
    ln(1.0 + u)
}

fn predict(model, x) {
    let value = model_value(model, x);
    let tail = model["tail"];
    let add = 0.0;
    for i in 0..tail["idx"].len() {
        let index = tail["idx"][i];
        let v = x[index];
        let maximum = tail["xMax"][i];
        if v > maximum && maximum > 0.0 {
            add += tail["coef"][i] * shape_value(v / maximum - 1.0, tail);
        }
    }
    let gained_add = add * tail["gain"];
    if gained_add > 0.0 && tail["maxAdd"] > 0.0 {
        value + if gained_add > tail["maxAdd"] { tail["maxAdd"] } else { gained_add }
    } else {
        value + if gained_add > 0.0 { gained_add } else { 0.0 }
    }
}

fn part_of(name) {
    if name.starts_with("g1_") { return "density"; }
    if name.starts_with("g2_") { return "stream"; }
    if name.starts_with("g3_") { return "jack"; }
    if name.starts_with("g4_") { return "chord"; }
    if name.starts_with("g5_") {
        return if name.contains("hand") { "reach" } else { "travel" };
    }
    if name.starts_with("g6_") { return "hold"; }
    if name.starts_with("g7_") { return "reading"; }
    if name.starts_with("g8_") { return "burst"; }
    if name.starts_with("g9_") { return "reach"; }
    if name.starts_with("g10_") { return "repeat"; }
    if name.starts_with("seg_handSpeedP95") || name.starts_with("seg_handTravel") || name.starts_with("seg_handJump") {
        return "reach";
    }
    if name.starts_with("seg_speedP95") { return "travel"; }
    if name.starts_with("seg_jackRatio") { return "jack"; }
    if name.starts_with("seg_chordRatio") { return "chord"; }
    if name.starts_with("seg_patternRepeat") { return "repeat"; }
    if name.starts_with("seg_burstMax") { return "burst"; }
    "density"
}

fn feature_index(names, wanted) {
    for i in 0..names.len() {
        if names[i] == wanted { return i; }
    }
    -1
}

fn percentile(distribution, value) {
    let low = 0;
    let high = distribution.len();
    while low < high {
        let mid = (low + high) / 2;
        if distribution[mid] <= value {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    if distribution.len() == 0 { 0.0 } else { low * 1.0 / distribution.len() }
}

fn on_chart_selected(ctx, settings) {
    let model = ctx.assets.model;
    let profile = ctx.assets.profile;
    let base_features = ctx.features;
    if base_features.len() != model.names.len() {
        throw "PPSR 特征版本与模型缓存不匹配";
    }

    let x = base_features;
    x.push(ctx.note_total);
    x.push(ctx.mech_total);
    x.push(ctx.duration);
    x.push(ctx.fake_ratio);
    x.push(ln(1.0 + ctx.note_total));
    x.push(ln(1.0 + ctx.duration));
    x.push(if ctx.duration > 0.0 { ctx.note_total / ctx.duration } else { 0.0 });

    let model_difficulty = predict(model, x);
    let difficulty = if ctx.chart_set == "phigros" && ctx.difficulty > 0.0 { ctx.difficulty } else { model_difficulty };
    let keys = ["density", "stream", "jack", "chord", "travel", "reach", "hold", "reading", "burst", "repeat"];
    let labels = ["底力", "间隔", "键型复杂度", "多指", "位移", "协调", "长条锁手", "读谱", "爆发", "重复度"];
    let headlines = [
        "g1_npsMech1sMax", "g2_streakMax", "g3_jackRunMax", "g4_chordMax", "g5_speedP95",
        "g9_handTravel2sMax", "g6_holdOverlapDensityMax", "g7_lineCount", "g8_burstMax", "g10_bigramRepeatRatio"
    ];
    let units = ["/s", " 连", " 连", " 键", " 屏/s", " 屏", "", " 条", " 键", "%"];
    let value_labels = ["峰值按键", "最长连打", "最长同指", "最大同押", "手速 P95", "2 秒最大行程", "重叠密度峰值", "判定线", "0.22s 内", "形态重复率"];
    let factors = [10.0, 1.0, 1.0, 1.0, 100.0, 100.0, 10.0, 1.0, 1.0, 10.0];
    let parts = [];
    let sum = 0.0;
    let median = profile.median;
    let median_notes = median["meta_noteTotal"];
    let median_mech = median["meta_mechTotal"];
    let median_duration = median["meta_duration"];
    let median_fake_ratio = median["meta_fakeRatio"];

    for i in 0..keys.len() {
        let key = keys[i];
        let response_x = [];
        for j in 0..model.augNames.len() {
            let name = model.augNames[j];
            if j < model.names.len() && part_of(name) == key {
                response_x.push(x[j]);
            } else if name == "x_noteTotal" {
                response_x.push(median_notes);
            } else if name == "x_mechTotal" {
                response_x.push(median_mech);
            } else if name == "x_duration" {
                response_x.push(median_duration);
            } else if name == "x_fakeRatio" {
                response_x.push(median_fake_ratio);
            } else if name == "x_logNoteTotal" {
                response_x.push(ln(1.0 + median_notes));
            } else if name == "x_logDuration" {
                response_x.push(ln(1.0 + median_duration));
            } else if name == "x_meanNps" {
                response_x.push(if median_duration > 0.0 { median_notes / median_duration } else { 0.0 });
            } else {
                response_x.push(median[name]);
            }
        }
        let response = predict(model, response_x);
        let distribution = profile.parts[key];
        let hold_index = feature_index(model.names, "g6_holdOverlapDensityMax");
        let absent = key == "hold" && x[hold_index] == 0.0;
        let rank = if absent { 0.0 } else { percentile(distribution, response) };
        let floor = if distribution.len() > 0 { distribution[0] } else { response };
        let contribute = if absent { floor - profile.base } else { response - profile.base };
        sum += contribute;

        let headline_index = feature_index(model.names, headlines[i]);
        let scale = if key == "repeat" { 100.0 } else { 1.0 };
        let value = x[headline_index] * scale;
        let factor = factors[i];
        let rounded_value = (value * factor + 0.5).floor() / factor;
        parts.push(#{
            key: key,
            label: labels[i],
            score: 1.0 + rank * 9.0,
            percentile: rank,
            contribute: contribute,
            absent: absent,
            absentHint: if absent { "这张谱没有任何重叠的长条，长条锁手无从谈起" } else { () },
            value: rounded_value,
            unit: units[i],
            valueLabel: value_labels[i]
        });
    }

    #{
        algorithm: "PPSR unified",
        difficulty: difficulty,
        parts: parts,
        baseline: profile.base,
        residual: difficulty - profile.base - sum,
        note_total: ctx.note_total,
        duration: ctx.duration
    }
}
