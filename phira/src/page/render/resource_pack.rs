//! Stable export choices from the existing imported-pack library.
use crate::data::Data;
use anyhow::{ensure, Context, Result};
use prpr::core::ResPackInfo;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Selection {
    #[default]
    FollowGame,
    Builtin,
    Imported(String),
}

pub(super) struct Choice {
    pub selection: Selection,
    pub label: String,
}

fn pack_path(root: &Path, id: &str) -> Result<std::path::PathBuf> {
    let mut components = Path::new(id).components();
    ensure!(matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(), "资源包标识无效，请重新选择渲染资源包");
    Ok(root.join(id))
}

fn info(root: &Path, id: &str) -> Result<ResPackInfo> {
    let file = std::fs::File::open(pack_path(root, id)?.join("info.yml")).context("资源包已移除或不可读取，请重新选择渲染资源包")?;
    let info: ResPackInfo = serde_yaml::from_reader(file).context("资源包信息无效，请重新选择渲染资源包")?;
    info.verify()?;
    Ok(info)
}

impl Selection {
    pub fn resolve(&self, data: &Data, root: &Path) -> Result<Option<String>> {
        let id = match self {
            Self::Builtin => return Ok(None),
            Self::FollowGame if data.respack_id == 0 => return Ok(None),
            Self::FollowGame => data
                .respacks
                .get(data.respack_id - 1)
                .context("游玩资源包已移除，请重新选择游玩或渲染资源包")?,
            Self::Imported(id) => {
                ensure!(data.respacks.contains(id), "渲染资源包已移除，请重新选择");
                id
            }
        };
        info(root, id)?;
        Ok(Some(pack_path(root, id)?.to_string_lossy().into_owned()))
    }
}

pub(super) fn choices(data: &Data, root: &Path, selected: &Selection) -> Vec<Choice> {
    let current = data
        .respack_id
        .checked_sub(1)
        .and_then(|i| data.respacks.get(i))
        .map(|id| info(root, id).map(|info| info.name).unwrap_or_else(|_| "资源包不可用".into()))
        .unwrap_or_else(|| "内置 Phira".into());
    let mut choices = vec![
        Choice {
            selection: Selection::FollowGame,
            label: format!("跟随游玩设置（{current}）"),
        },
        Choice {
            selection: Selection::Builtin,
            label: "内置默认（Phira）".into(),
        },
    ];
    for id in &data.respacks {
        if let Ok(info) = info(root, id) {
            choices.push(Choice {
                selection: Selection::Imported(id.clone()),
                label: info.name,
            });
        }
    }
    if !choices.iter().any(|choice| &choice.selection == selected) {
        choices.push(Choice {
            selection: selected.clone(),
            label: "已选资源包不可用，请重新选择".into(),
        });
    }
    choices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_are_stable_independent_and_missing_packs_do_not_fall_back() {
        let root = tempfile::tempdir().unwrap();
        for (id, name) in [("skin-a", "Skin A"), ("skin-b", "Skin B")] {
            let pack = root.path().join(id);
            std::fs::create_dir(&pack).unwrap();
            let mut info: serde_yaml::Value = serde_yaml::from_str(include_str!("../../../../assets/respack/info.yml")).unwrap();
            info["name"] = name.into();
            std::fs::write(pack.join("info.yml"), serde_yaml::to_string(&info).unwrap()).unwrap();
        }
        let mut data = Data::default();
        data.respacks = vec!["skin-a".into(), "skin-b".into()];
        data.respack_id = 1;
        data.config.res_pack_path = Some("unused-stale-selection".into());
        let follow = Selection::FollowGame;
        let fixed = Selection::Imported("skin-b".into());
        assert_eq!(follow.resolve(&data, root.path()).unwrap(), Some(root.path().join("skin-a").to_string_lossy().into_owned()));
        assert_eq!(Selection::Builtin.resolve(&data, root.path()).unwrap(), None);
        let expected = fixed.resolve(&data, root.path()).unwrap();
        data.respacks.swap(0, 1);
        assert_eq!(fixed.resolve(&data, root.path()).unwrap(), expected);
        assert_eq!(data.respack_id, 1);
        assert_eq!(data.config.res_pack_path.as_deref(), Some("unused-stale-selection"));
        data.respack_id = 0;
        assert_eq!(follow.resolve(&data, root.path()).unwrap(), None);
        std::fs::remove_file(root.path().join("skin-b/info.yml")).unwrap();
        assert!(fixed.resolve(&data, root.path()).is_err());
        assert!(choices(&data, root.path(), &fixed).last().unwrap().label.contains("不可用"));
        data.respacks.clear();
        assert!(fixed.resolve(&data, root.path()).is_err());
        assert!(pack_path(root.path(), "../skin").is_err());
    }
}
