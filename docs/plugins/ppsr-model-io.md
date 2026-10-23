# PPSR v2 model input/output contract

This contract applies to the replaceable `model.json` and `profile.json` assets in the PPSR v2 plugin. It is intentionally independent of the training service: the plugin performs inference only and does not download, train, update, or persist a model.

## Feature extraction ownership

Plugin API v2 supports algorithm-specific feature extraction: `on_chart_selected` receives the local chart's parsed JSON object as `ctx.chart_data` when the source is at most 4 MiB. A plugin may parse it and build any feature vector in its own Rhai script; feature names and dimensions are not shared between plugins. The [PDE-R example](./examples/pde-r/) demonstrates a separate 106-feature algorithm and model.

This PPSR v2 package currently uses a legacy compatibility path: the host creates its exact 187-value vector and the plugin performs model inference and breakdown mapping. The host only runs that PPSR extractor when `phira-pro.ppsr` is enabled; unrelated plugins receive `chart_data` without PPSR feature computation. Thus replacing this package's model/profile does not replace the PPSR feature algorithm. Moving PPSR's 187-feature implementation into its own Rhai package remains separate work.

## Input vector

In addition to `chart_data`, the legacy PPSR callback receives:

```json
{
  "feature_version": 2,
  "v2_feature_version": 1,
  "feature_names": ["..."],
  "features": [0.0],
  "note_total": 0,
  "mech_total": 0,
  "duration": 0.0,
  "fake_ratio": 0.0,
  "difficulty": 0.0,
  "chart_set": "phira"
}
```

`features` and `feature_names` are parallel arrays of exactly 187 items. The receiver must use the order below verbatim; it must not sort the names. The plugin rejects a wrong count, feature version, or name at any index.

1. **Base features (114):** the names in the existing PPSR unified `modelFeatureNames` order, reproduced at the start of the model's `names` array.
2. **PPSR v2 chart features (66):** hold occupancy and movement (23), four-finger load (9), per-hand patterns (8), beat and subdivision (9), pattern/section structure (11), and density distribution (6).
3. **Model context features (7):** `x_noteTotal`, `x_mechTotal`, `x_duration`, `x_fakeRatio`, `x_logNoteTotal`, `x_logDuration`, `x_meanNps`.

The exact authoritative 187-name sequence is:

```text
g1_npsAvg, g1_npsMechAvg, g1_nps1sMax, g1_npsMech1sMax, g1_maxNotes2s, g1_nps2sMax, g1_maxNotes4s, g1_nps4sMax, g1_gapRatio, g1_activeRatio, g1_highDensityRatio, g2_dtMin, g2_dtP10, g2_dtP50, g2_streakMax, g2_streakNoteRatio, g3_jackRunMax, g3_jackNoteRatio, g3_trillRunMax, g4_chordMax, g4_chordNoteRatio, g4_chord3NoteRatio, g4_chordPerSecMax, g5_distP50, g5_distP90, g5_speedP95, g5_speedMax, g5_crossScreenRatio, g5_beyondToleranceRatio, g5_handTravelP95, g5_handTravelMax, g5_handSpeedP95, g5_handOverreachRatio, g5_sameFingerRepeatMax, g6_holdNoteRatio, g6_holdTimeRatio, g6_holdLenMean, g6_holdLenMax, g6_holdOverlapMax, g6_holdOverlapDensityMax, g6_holdOverlapAvg, g6_holdMovingRatio, g7_flickRatio, g7_dragRatio, g7_fakeRatio, g7_lineCount, g7_movingLineRatio, g7_lineTravelXAvg, g7_lineTravelXMax, g7_lineRotAmpAvg, g7_lineRotAmpMax, g8_perfectRatio, g8_goodRatio, g8_badRatio, g8_dtWindowP10, g8_tightJackRatio, g8_burstMax, g8_burstP95, g9_handSpeedP99, g9_handSpeedMax, g9_handExcessMean, g9_handExcessP90, g9_handImpossibleRatio, g9_handStepP95, g9_handStepMax, g9_handTravel2sMax, g9_handJump2sMax, g9_crossSideRatio, g9_sameSideFastRatio, g9_dirChangeRatio, g9_sideChordMax, g9_sideChord3Ratio, g10_bigramRepeatRatio, g10_trigramRepeatRatio, g10_distinctShapeRatio, g10_sameIntervalRatio, g10_sameShapeRatio, g10_period2RunMax, seg_npsMean, seg_npsP90, seg_npsMax, seg_mechNpsMean, seg_mechNpsP90, seg_mechNpsMax, seg_speedP95Mean, seg_speedP95P90, seg_speedP95Max, seg_jackRatioMean, seg_jackRatioP90, seg_jackRatioMax, seg_chordRatioMean, seg_chordRatioP90, seg_chordRatioMax, seg_handSpeedP95Mean, seg_handSpeedP95P90, seg_handSpeedP95Max, seg_handTravelMean, seg_handTravelP90, seg_handTravelMax, seg_handJumpMean, seg_handJumpP90, seg_handJumpMax, seg_patternRepeatMean, seg_patternRepeatP90, seg_patternRepeatMax, seg_burstMaxMean, seg_burstMaxP90, seg_burstMaxMax, seg_peakNps, seg_peakPosition, seg_activeRatio, seg_headNps, seg_tailNps, seg_tailHeadRatio, h_holdCount, h_holdPerSec, h_holdLenMean, h_holdLenP90, h_holdLenMax, h_holdConcurrentMax, h_holdConcurrentMean, h_holdConcurrentP90, h_holdBusyRatio, h_holdMultiRatio, h_holdTripleRatio, h_holdStartChordMax, h_holdReleasePerSec, h_holdReleaseChordMax, h_holdTapDuringRatio, h_holdTapOverlapMax, h_holdMoveRatio, h_holdMoveSpeedP95, h_holdMoveSpeedMax, h_holdPathPerSec, h_holdMovingConcurrentMax, h_holdMovingTimeRatio, h_holdEndDropP90, f_forcedRatio, f_freeFingersMin, f_busyFingersMax, f_busyFingersMean, f_holdHogRatio, f_travelP95, f_travelMax, f_speedP95, f_speedMax, p_handNpsMax, p_handNpsMin, p_handBalance, p_handChordMax, p_sameHandStreamMax, p_sameHandJackRatio, p_handSpeedP95Max, p_handTravelPerSec, b_bpm, b_bpmChanges, b_beatIntervalP50, b_beatIntervalP90, b_beatIntervalCv, b_offGridRatio, b_subdivEntropy, b_notePerBeatMax, b_longHoldCrossBeatRatio, pat_entropy, pat_bigramEntropy, pat_trigramEntropy, pat_4gramRepeatRatio, pat_8gramRepeatRatio, pat_sectionRepeatRatio, pat_sectionDistinctRatio, pat_periodicity, pat_periodLag, pat_kindEntropy, pat_chordEntropy, d_npsEntropy, d_npsGini, d_top10sShare, d_peakCount, d_trend, d_burstP99, x_noteTotal, x_mechTotal, x_duration, x_fakeRatio, x_logNoteTotal, x_logDuration, x_meanNps
```

All chart features are finite numeric values rounded to four decimal places. Context values are sourced from the same extracted chart: `x_logNoteTotal = ln(1 + note_total)`, `x_logDuration = ln(1 + duration)`, and `x_meanNps = note_total / duration` when duration is positive. The plugin's extractor does not see trailing silence or a later fake note when the parsed chart type does not retain raw duration; in that case `duration` uses the final non-fake note end.

## `model.json`

The root object must use `format: "phira-ppsr-model"`, `version: 2`, `featureVersion: 2`, and `v2FeatureVersion: 1`. It contains:

| Field | Contract |
| --- | --- |
| `modelTag` | Stable identifier shared by the model and its profile. |
| `names` | The exact 187-name vector above. |
| `baseNames` | The first 114 names, retained for model provenance. |
| `bag.models` | Non-empty array of learners. Each learner has `base`, `lr`, `featureCount: 187`, and `trees`. |
| `deshrink` | `{ "slope": number, "intercept": number }`. |
| `tail` | Tail extrapolation fields `idx`, `xMax`, `coef`, `gain`, `maxAdd`, `shape`, and `expK`. The three arrays are index-aligned. |

Each tree node is either a leaf `{ "leaf": number }`, or a split `{ "feature": integer, "threshold": number, "left": node, "right": node }`. A split goes left when `x[feature] <= threshold`, otherwise right. A learner prediction is `base + lr * sum(tree leaf values)`. The raw prediction is the arithmetic mean of learner predictions. Apply `deshrink.slope * raw + deshrink.intercept`, then add the tail increment. For each tail index whose input is above its positive `xMax`, the increment is `coef * shape(input / xMax - 1)`, where `shape` is `log(1 + u)` for `"log"`, `u` for `"linear"`, or `(exp(expK * u) - 1) / expK` for `"exp"`; multiply the sum by `gain`, clamp it to at least zero, and apply the positive `maxAdd` cap when present.

Do not change tree representation, learner averaging, deshrink order, tail handling, or feature order when replacing the model. The selected-chart callback uses the user-configurable Rhai operation budget, defaulting to 5,000,000 operations; `on_frame` remains fixed at 25,000.

## `profile.json`

The profile is model-specific and must be replaced whenever `modelTag`, `names`, feature versions, or model predictions change. Required fields:

```json
{
  "format": "phira-ppsr-profile",
  "version": 1,
  "featureVersion": 2,
  "v2FeatureVersion": 1,
  "modelTag": "same value as model.json",
  "names": ["same 187 names in the same order"],
  "partFeatures": { "density": ["..."], "stream": ["..."] },
  "count": 1458,
  "base": 0.0,
  "median": { "feature-name": 0.0 },
  "features": { "feature-name": [0.0] },
  "directions": { "feature-name": 1 },
  "parts": { "density": [0.0], "stream": [0.0] }
}
```

`median` contains one finite number for each of the 187 model names. `base` is the model prediction on that median vector. `features` contains one non-empty ascending empirical value array for every model name; `directions` contains `1` or `-1` for every name and indicates whether increasing values retain or reverse their empirical rank. `partFeatures` records the partition of the 180 chart features. The seven `x_*` model-context features remain neutral and are not assigned to a difficulty dimension.

To calculate a part measure, compute each member feature's empirical rank with an upper-bound search, apply its direction (`1 - rank` for direction `-1`), then average the adjusted ranks for that part. Each `parts[key]` is the non-empty ascending distribution of these aggregate measures across the feature-pool samples. The plugin looks up the current aggregate measure in that distribution; the displayed score is `1 + 9 * percentile`. A chart with no holds is assigned rank zero for the long-hold part. The current ten output keys, in stable order, are `density`, `stream`, `jack`, `chord`, `travel`, `reach`, `hold`, `reading`, `burst`, and `repeat`.

## Output

The PPSR hook returns:

```json
{
  "algorithm": "PPSR v2",
  "model": "modelTag",
  "difficulty": 12.34,
  "source": "model",
  "parts": [{
    "key": "hold",
    "label": "长条占用",
    "score": 5.5,
    "percentile": 0.5,
    "contribute": 0.0,
    "absent": false,
    "value": 2.0,
    "unit": "条",
    "valueLabel": "最大同时按住"
  }],
  "baseline": 13.59,
  "note_total": 100,
  "duration": 50.0
}
```

`source` is `"official"` with the positive official difficulty for an in-built `phigros` chart; otherwise it is `"model"` and `difficulty` is the fitted model result. The reported difficulty is not multiplied by an additional scale factor. `parts` always has ten items; the labels explain gameplay-facing dimensions, while `value` / `unit` / `valueLabel` identify a representative measurable feature. `contribute` is the dimension's empirical percentile centered on the profile midpoint (`2 * (percentile - 0.5)`), not a model difficulty-point contribution. `baseline` is the fitted model's output for the profile median vector; it is informational and is not added to the ten part scores.

The packaged model and profile are UTF-8 JSON root entries declared in the plugin Manifest. A replacement package must stay under the 2 MiB ZIP limit, 8 MiB per JSON resource, 8 MiB merged assets, and 10 MiB total expanded content. Keep the paired model/profile files from one build, validate all 187 names, and run the plugin tests before distributing a replacement.
