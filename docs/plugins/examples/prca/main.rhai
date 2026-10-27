// Rules remain in the independently installed plugin. Missing capabilities are
// reported per check; they are never evidence of cheating.
fn evidence(family, title, detail, start, end) {
    #{ family: family, title: title, detail: detail, start: start, end: end }
}
fn check(name, status, samples, needed, reason) {
    #{ name:name, status:status, samples:samples, needed:needed, reason:reason }
}
fn on_replay_analysis(ctx, settings) {
    if ctx.feature_schema != "replay-observations-1" || !ctx.contains("compact") {
        throw "PRCA 0.3 需要新版主程序，请同时更新主程序和插件";
    }
    let q=ctx.quality;
    let p=ctx.assets.profile;
    let c=ctx.compact;
    let e=[];
    let checks=[];
    let conflicts=ctx.chart.verified && (ctx.metrics.invalid_targets>0 || ctx.metrics.duplicate_stages>0);
    checks.push(check("谱面与判定一致性", if conflicts {"data"} else if ctx.chart.verified {"pass"} else {"unavailable"},
        ctx.coverage.judgement_events,1,if conflicts {"存在引用或重复记录问题，需核对录制器。"}
        else if ctx.chart.verified {"已校验录制谱面，未发现结构冲突。"} else {"未匹配录制版本，此项目跳过。"}));
    if conflicts {
        e.push(evidence("data","判定记录需要核对",
            "无效引用 "+ctx.metrics.invalid_targets+" 条，重复阶段 "+ctx.metrics.duplicate_stages+" 条。数据问题不作为玩家作弊证据。",0.0,ctx.metadata.duration));
    }
    let clean_raw=q.raw_input && q.dropped==0 && q.time_reversals==0 && q.clock_mismatch==0 && !q.repaired_order;
    let keyboard_timing=clean_raw && q.primary_source=="keyboard" && q.timing_available && q.judgement_config.algorithm=="phiraPro";
    let timing_count=0;
    let timing_first=#{};
    let timing_last=#{};
    for page in 0..((c.timing_blocks+127)/128) {
        for w in replay_observations("timing",page*128,128) {
            if (q.timing_candidate_eligible || keyboard_timing) && w.stats.n>=p.block_samples &&
                w.stats.mad<=p.timing_mad_ms && w.stats.p95-w.stats.p05<=p.timing_span_ms {
                if timing_count==0 { timing_first=w; }
                timing_last=w;
                timing_count+=1;
            }
        }
    }
    let by_key=#{};
    for page in 0..((c.duration_blocks+127)/128) {
        for w in replay_observations("duration",page*128,128) {
            if q.duration_candidate_eligible && w.stats.n>=p.block_samples &&
                w.stats.mode_ratio>=p.duration_mode_ratio && w.stats.mode_lower95>=p.duration_mode_lower95 &&
                w.intervals.n>=p.block_samples-1 && w.intervals.p95-w.intervals.p05>=p.min_interval_span_ms &&
                (q.backend_timestamp_fraction>=0.95 || w.stats.frame_lattice_ratio<p.frame_lattice_ratio) {
                // A stable peak must recur on the same input channel. Keep
                // summaries only, never accumulate an entire replay in Rhai.
                let key=w.key+":"+w.stats.median.round().to_int();
                if !by_key.contains(key) { by_key[key]=#{count:0,first:w,last:w}; }
                by_key[key].count+=1;
                by_key[key].last=w;
            }
        }
    }
    let duration=#{count:0};
    for key in by_key.keys() {
        if by_key[key].count>duration.count { duration=by_key[key]; }
    }
    let timing_review=q.timing_candidate_eligible && timing_count>=p.min_repeated_windows;
    let duration_review=duration.count>=p.min_repeated_windows &&
        (q.backend_timestamp_fraction>=0.95 || duration.count>=p.keyboard_duration_windows || timing_count>=p.min_repeated_windows);
    if timing_review {
        let w=timing_first;
        e.push(evidence("timing","多个有效样本块的命中偏差异常集中",
            "发现 "+timing_count+" 个不重叠样本块，首块 "+w.stats.n+" 次有效命中，MAD "+w.stats.mad+
            " ms。同帧和弦只计一次；仍需结合设备和现场操作复核。",w.start,timing_last.end));
    }
    if duration_review {
        let w=duration.first;
        e.push(evidence("duration","同一输入通道的按压时长反复集中",
            "同一通道有 "+duration.count+" 个不重叠样本块，按下间隔存在变化而时长峰持续重复。首块 "+w.stats.n+
            " 次完整按压，峰占比 "+w.stats.mode_ratio+"，二项近似比例下界 "+w.stats.mode_lower95+
            "。已排除明显帧周期量化；仍需复核硬件和操作方式。",w.start,duration.last.end));
    }
    let timing_basic=q.timing_available && c.independent_timing_samples>=p.basic_samples;
    let duration_basic=c.complete_contacts>=p.basic_samples;
    checks.push(check("命中计时",if timing_review {"review"} else if timing_basic {if timing_count>0 || (!q.timing_candidate_eligible && !keyboard_timing) {"observe"} else {"pass"}}
        else if q.timing_available {"insufficient"} else {"unavailable"}, c.independent_timing_samples,p.basic_samples,
        if timing_review {"多个样本块出现集中偏差，需要复核。"}
        else if timing_basic {if keyboard_timing {"键盘计时可用；细微集中需其他证据支持。"} else {"有效命中数据已检查；不支持的精细推断已跳过。"}}
        else if q.timing_available {"基础计时样本较少，已保留观测。"} else {"缺少可核对的谱面、误差或判定配置。"}));
    checks.push(check("按压时长",if duration_review {"review"} else if duration_basic {if duration.count>0 || !q.duration_candidate_eligible {"observe"} else {"pass"}}
        else if c.complete_contacts>0 {"insufficient"} else {"unavailable"},c.complete_contacts,p.basic_samples,
        if duration_review {"同一通道反复出现窄峰，需要复核。"}
        else if duration_basic {if !q.duration_candidate_eligible {"仅提供时长观测；记录精度不足，精细检测未启用。"} else if q.primary_source=="keyboard" {"键盘逐键检查，排除辅助鼠标与帧量化影响。"} else {"完整接触时长已检查；取消和截尾接触不计入。"}}
        else {"完整按压较少，不将缺口或短谱面视为嫌疑。"}));
    checks.push(check("触点落点与位移",if q.primary_source=="keyboard" {"not_applicable"} else if q.geometry_available {"observe"} else {"unavailable"},
        ctx.coverage.geometry_samples,0,if q.primary_source=="keyboard" {"键盘模式不适用触点位置检查。"}
        else if q.geometry_available {"提供实测候选图，不单独用速度或位移定性。"} else {"此回放不具备可核对的触点几何数据。"}));
    let enough=timing_basic || duration_basic;
    let status=if conflicts {"conflict"} else if e.len>0 {"review"} else if enough {"clear"} else {"limited"};
    let summary=if conflicts {"录制数据需要核对，尚未形成玩家作弊嫌疑。"}
        else if e.len>0 {"发现可定位的重复异常特征，需要复核。"}
        else if enough {"已完成可用项目检查，未发现需要复核的异常。"}
        else {"有效操作较少，仅提供观测，不列为作弊嫌疑。"};
    #{status:status,summary:summary,experimental:true,profile_version:p.version,calibrated:false,evidence:e,checks:checks,
        limitations:"正常表示本次可用检查未发现异常；未校准的统计特征不认证清白或作弊。"}
}