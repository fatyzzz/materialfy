//! Минимальные изменения модели очереди: старые строки → новые (по uid). Строка с тем же uid, но другим
//! содержимым, уходит и приходит заново. Слишком много шагов — один Reset.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use md3_model::QueueRow;

use crate::QueueChange;

const MAX_STEPS: usize = 24;

pub fn queue_changes(old: &[QueueRow], new: &Arc<Vec<QueueRow>>) -> Vec<QueueChange> {
    if old == new.as_slice() {
        return Vec::new();
    }
    // пошаговый дифф опирается на уникальность uid; повтор (или пустой список) — просто Reset
    if old.is_empty() || new.is_empty() || !unique_uids(old) || !unique_uids(new) {
        return vec![QueueChange::Reset(new.clone())];
    }
    let fresh: HashMap<u64, &QueueRow> = new.iter().map(|row| (row.uid, row)).collect();
    // строки, которые остаются как есть
    let kept: HashSet<u64> = old
        .iter()
        .filter(|row| fresh.get(&row.uid).is_some_and(|other| *other == *row))
        .map(|row| row.uid)
        .collect();
    let mut steps = Vec::new();
    let mut current: Vec<u64> = old.iter().map(|row| row.uid).collect();

    // 1. удаления (подряд идущие — одним шагом)
    let mut at = 0;
    while at < current.len() {
        if kept.contains(&current[at]) {
            at += 1;
            continue;
        }
        let mut count = 1;
        while at + count < current.len() && !kept.contains(&current[at + count]) {
            count += 1;
        }
        current.drain(at..at + count);
        steps.push(QueueChange::Remove { at, count });
    }

    // 2. перестановки: всё вне наибольшей возрастающей подпоследовательности
    let target: Vec<u64> = new.iter().map(|row| row.uid).filter(|uid| kept.contains(uid)).collect();
    if current != target {
        let position: HashMap<u64, usize> = current.iter().enumerate().map(|(i, uid)| (*uid, i)).collect();
        let Some(order) = target.iter().map(|uid| position.get(uid).copied()).collect::<Option<Vec<usize>>>() else {
            return vec![QueueChange::Reset(new.clone())];
        };
        let stay = longest_increasing(&order);
        for (index, uid) in target.iter().enumerate() {
            if stay.contains(&index) {
                continue;
            }
            let Some(from) = current.iter().position(|other| other == uid) else {
                return vec![QueueChange::Reset(new.clone())];
            };
            current.remove(from);
            let to = if index == 0 {
                0
            } else {
                match current.iter().position(|other| *other == target[index - 1]) {
                    Some(at) => at + 1,
                    None => return vec![QueueChange::Reset(new.clone())],
                }
            };
            current.insert(to, *uid);
            if from != to {
                steps.push(QueueChange::Move { from, to });
            }
        }
    }

    // 3. вставки по порядку
    let mut index = 0;
    while index < new.len() {
        if kept.contains(&new[index].uid) {
            index += 1;
            continue;
        }
        let start = index;
        while index < new.len() && !kept.contains(&new[index].uid) {
            index += 1;
        }
        steps.push(QueueChange::Insert { at: start, rows: new[start..index].to_vec() });
    }

    if steps.len() > MAX_STEPS { vec![QueueChange::Reset(new.clone())] } else { steps }
}

fn unique_uids(rows: &[QueueRow]) -> bool {
    let mut seen = HashSet::with_capacity(rows.len());
    rows.iter().all(|row| seen.insert(row.uid))
}

/// Индексы (в `values`) наибольшей возрастающей подпоследовательности.
fn longest_increasing(values: &[usize]) -> HashSet<usize> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![usize::MAX; values.len()];
    for (i, value) in values.iter().enumerate() {
        let at = tails.partition_point(|&tail| values[tail] < *value);
        if at > 0 {
            previous[i] = tails[at - 1];
        }
        if at == tails.len() {
            tails.push(i);
        } else {
            tails[at] = i;
        }
    }
    let mut out = HashSet::new();
    let mut cursor = tails.last().copied();
    while let Some(i) = cursor {
        out.insert(i);
        cursor = (previous[i] != usize::MAX).then_some(previous[i]);
    }
    out
}

/// Применить изменения к списку так же, как это делает модель моста (для тестов).
#[cfg(test)]
pub fn apply(rows: &mut Vec<QueueRow>, changes: &[QueueChange]) {
    for change in changes {
        match change {
            QueueChange::Reset(all) => *rows = all.as_ref().clone(),
            QueueChange::Insert { at, rows: added } => {
                let at = (*at).min(rows.len());
                rows.splice(at..at, added.iter().cloned());
            }
            QueueChange::Remove { at, count } => {
                let end = (at + count).min(rows.len());
                rows.drain(*at..end);
            }
            QueueChange::Move { from, to } => {
                let row = rows.remove(*from);
                rows.insert(*to, row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use md3_model::{QueueSection, TrackItem};

    fn row(uid: u64, section: QueueSection) -> QueueRow {
        QueueRow { section, uid, recommended: false, track: TrackItem { uri: format!("spotify:track:{uid}"), ..Default::default() } }
    }

    fn rows(uids: &[u64]) -> Vec<QueueRow> {
        uids.iter().map(|uid| row(*uid, QueueSection::Context)).collect()
    }

    fn check(old: &[u64], new: &[u64]) -> usize {
        let old = rows(old);
        let new = Arc::new(rows(new));
        let changes = queue_changes(&old, &new);
        let mut applied = old.clone();
        apply(&mut applied, &changes);
        assert_eq!(applied, *new, "{changes:?}");
        changes.len()
    }

    #[test]
    fn the_steps_rebuild_the_new_queue() {
        assert_eq!(check(&[1, 2, 3], &[1, 2, 3]), 0);
        assert_eq!(check(&[1, 2, 3], &[2, 3]), 1);
        assert_eq!(check(&[1, 2, 3], &[1, 2, 3, 4, 5]), 1);
        assert_eq!(check(&[1, 2, 3, 4], &[2, 3, 4, 1]), 1, "one move down is one step");
        assert_eq!(check(&[1, 2, 3, 4], &[4, 1, 2, 3]), 1);
        check(&[1, 2, 3, 4, 5], &[9, 5, 3, 8, 1]);
        check(&[1, 2, 3], &[7, 8]);
        check(&[], &[1]);
        check(&[1], &[]);
    }

    #[test]
    fn a_row_whose_section_changed_is_replaced() {
        let old = vec![row(1, QueueSection::Current), row(2, QueueSection::Manual), row(3, QueueSection::Context)];
        let new = Arc::new(vec![row(2, QueueSection::Current), row(3, QueueSection::Context)]);
        let changes = queue_changes(&old, &new);
        let mut applied = old.clone();
        apply(&mut applied, &changes);
        assert_eq!(applied, *new);
    }

    #[test]
    fn repeated_uids_fall_back_to_reset() {
        for (old, new) in [
            (&[1, 2, 3][..], &[3, 1, 1, 2][..]),
            (&[1, 2, 2, 3][..], &[3, 2, 1][..]),
            (&[1, 1][..], &[1, 1][..]),
            (&[4, 4, 5][..], &[5, 4, 4, 6][..]),
        ] {
            let old = rows(old);
            let new = Arc::new(rows(new));
            let changes = queue_changes(&old, &new);
            let mut applied = old.clone();
            apply(&mut applied, &changes);
            assert_eq!(applied, *new, "{changes:?}");
            assert!(changes.iter().all(|c| matches!(c, QueueChange::Reset(_))) || changes.is_empty(), "{changes:?}");
        }
    }
}
