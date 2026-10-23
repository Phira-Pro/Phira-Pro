fn tree_value(tree, x) {
    let node = tree;
    while !node.contains("leaf") {
        let index = node["feature"];
        node = if x[index] <= node["threshold"] { node["left"] } else { node["right"] };
    }
    node["leaf"]
}

fn raw_value(model, x) {
    let models = model["bag"]["models"];
    if models.len() == 0 { throw "PPSR 模型没有 bag learner"; }
    let total = 0.0;
    for model_index in 0..models.len() {
        let learner = models[model_index];
        if learner["featureCount"] != x.len() { throw "PPSR learner 特征数不匹配"; }
        let value = learner["base"];
        for tree in learner["trees"] {
            value += learner["lr"] * tree_value(tree, x);
        }
        total += value;
    }
    total / models.len()
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
    let raw = raw_value(model, x);
    let deshrink = model["deshrink"];
    let value = deshrink["slope"] * raw + deshrink["intercept"];
    let tail = model["tail"];
    let add = 0.0;
    for i in 0..tail["idx"].len() {
        let index = tail["idx"][i];
        let feature = x[index];
        let maximum = tail["xMax"][i];
        if feature > maximum && maximum > 0.0 {
            add += tail["coef"][i] * shape_value(feature / maximum - 1.0, tail);
        }
    }
    let gained_add = add * tail["gain"];
    if gained_add > 0.0 {
        value + if tail["maxAdd"] > 0.0 && gained_add > tail["maxAdd"] { tail["maxAdd"] } else { gained_add }
    } else {
        value
    }
}

fn part_of(name) {
    if name.starts_with("g1_") || (name.starts_with("d_") && name != "d_burstP99") { return "density"; }
    if name.starts_with("g2_") || name.starts_with("p_handNps") || name == "p_handBalance" || name == "p_sameHandStreamMax" {
        return "stream";
    }
    if name.starts_with("g3_") || name == "p_sameHandJackRatio" { return "jack"; }
    if name.starts_with("g4_") || name == "p_handChordMax" || name.starts_with("h_holdStartChord") ||
        name == "h_holdReleaseChordMax" || name == "pat_chordEntropy" {
        return "chord";
    }
    if name.starts_with("g5_") && !name.contains("hand") || name.starts_with("seg_speedP95") ||
        name == "f_travelP95" || name == "f_travelMax" || name == "f_speedP95" || name == "f_speedMax" ||
        name.starts_with("h_holdMove") || name == "h_holdPathPerSec" {
        return "travel";
    }
    if name.starts_with("g5_hand") || name.starts_with("g9_") ||
        name.starts_with("seg_handSpeedP95") || name.starts_with("seg_handTravel") || name.starts_with("seg_handJump") ||
        name.starts_with("f_") || name == "p_handSpeedP95Max" || name == "p_handTravelPerSec" {
        return "reach";
    }
    if name.starts_with("g6_") || name.starts_with("h_") || name == "b_longHoldCrossBeatRatio" {
        return "hold";
    }
    if name.starts_with("g7_") || name.starts_with("b_") { return "reading"; }
    if name.starts_with("g8_") || name == "d_burstP99" { return "burst"; }
    if name.starts_with("g10_") || name.starts_with("pat_") || name.starts_with("seg_patternRepeat") {
        return "repeat";
    }
    if name.starts_with("seg_jackRatio") { return "jack"; }
    if name.starts_with("seg_chordRatio") { return "chord"; }
    if name.starts_with("seg_burstMax") { return "burst"; }
    if name.starts_with("seg_") { return "density"; }
    "baseline"
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
    let x = ctx.features;
    let names = model.names;
    if model.format != "phira-ppsr-model" || model.version != 2 ||
        ctx.feature_version != model.featureVersion || ctx.v2_feature_version != model.v2FeatureVersion ||
        x.len() != names.len() || names.len() != 187 || ctx.feature_names.len() != names.len() {
        throw "PPSR 特征版本或维数与模型缓存不匹配";
    }
    if profile.format != "phira-ppsr-profile" || profile.version != 1 ||
        profile.featureVersion != model.featureVersion || profile.v2FeatureVersion != model.v2FeatureVersion ||
        profile.modelTag != model.modelTag || profile.names.len() != names.len() {
        throw "PPSR 分项画像与模型版本不匹配";
    }
    for i in 0..names.len() {
        if ctx.feature_names[i] != names[i] || profile.names[i] != names[i] {
            throw "PPSR 模型或分项画像特征顺序与宿主不匹配";
        }
    }

    let model_difficulty = predict(model, x);
    let official = ctx.chart_set == "phigros" && ctx.difficulty > 0.0;
    let difficulty = if official { ctx.difficulty } else { model_difficulty };
    let keys = ["density", "stream", "jack", "chord", "travel", "reach", "hold", "reading", "burst", "repeat"];
    let labels = ["谱面密度 / 底力", "纵连与交互", "Jack / Trill", "多押与和弦", "位移与手速", "手序与指位",
        "长条占用", "节奏切分", "短时爆发", "谱型重复"];
    let headlines = ["g1_npsMech1sMax", "g2_streakMax", "g3_jackRunMax", "g4_chordMax", "g5_speedP95",
        "g9_handTravel2sMax", "h_holdConcurrentMax", "b_offGridRatio", "g8_burstMax", "pat_4gramRepeatRatio"];
    let units = ["/s", " 连", " 连", " 键", " 屏/s", " 屏", " 条", "%", " 键 / 0.22s", "%"];
    let value_labels = ["机械键峰值", "最长连续段", "最长同指段", "最大同刻多押", "位移速度 P95", "2 秒最大手部行程",
        "最大同时按住", "非四分拍网格率", "0.22 秒窗口峰值", "4-gram 重复率"];
    let scales = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 100.0, 1.0, 100.0];
    let sums = #{
        density: 0.0, stream: 0.0, jack: 0.0, chord: 0.0, travel: 0.0,
        reach: 0.0, hold: 0.0, reading: 0.0, burst: 0.0, repeat: 0.0
    };
    let counts = #{
        density: 0, stream: 0, jack: 0, chord: 0, travel: 0,
        reach: 0, hold: 0, reading: 0, burst: 0, repeat: 0
    };
    for j in 0..names.len() {
        let key = part_of(names[j]);
        if key != "baseline" {
            let distribution = profile.features[names[j]];
            if distribution.len() == 0 { throw "PPSR 分项特征画像为空"; }
            let raw_rank = percentile(distribution, x[j]);
            let direction = profile.directions[names[j]];
            sums[key] += if direction < 0 { 1.0 - raw_rank } else { raw_rank };
            counts[key] += 1;
        }
    }
    let parts = [];
    for i in 0..keys.len() {
        let key = keys[i];
        let measure = sums[key] / counts[key];
        let distribution = profile.parts[key];
        let hold_index = feature_index(names, "h_holdCount");
        let absent = key == "hold" && x[hold_index] == 0.0;
        let rank = if absent { 0.0 } else { percentile(distribution, measure) };
        let value_index = feature_index(names, headlines[i]);
        let scale = scales[i];
        let shown_value = (x[value_index] * scale * 10.0 + 0.5).floor() / 10.0;
        parts.push(#{
            key: key,
            label: labels[i],
            score: 1.0 + rank * 9.0,
            percentile: rank,
            contribute: (rank - 0.5) * 2.0,
            absent: absent,
            value: shown_value,
            unit: units[i],
            valueLabel: value_labels[i]
        });
    }
    #{
        algorithm: "PPSR v2",
        model: model.modelTag,
        difficulty: difficulty,
        source: if official { "official" } else { "model" },
        parts: parts,
        baseline: profile.base,
        note_total: ctx.note_total,
        duration: ctx.duration
    }
}
