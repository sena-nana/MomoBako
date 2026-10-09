//! 按键写回 Store 列表的回归：增、删、改、重排、重复键各自算出的操作，写完的列表和新列表一样，
//! 结构操作不让留下的行重算，改了的行才重算。

use std::cell::Cell;
use std::rc::Rc;

use nana_ui::runtime::view::{computed, store, StoreList, StorePath};

use super::{plan_rows, sync_rows, RowPlan};

type Row = (u32, &'static str);

fn key(row: &Row) -> u32 {
    row.0
}

fn plan(remove: bool, insert: &[usize], change: &[usize], reorder: bool) -> Option<RowPlan> {
    Some(RowPlan { remove, insert: insert.to_vec(), change: change.to_vec(), reorder })
}

#[test]
fn plans_follow_keys_contents_and_order() {
    let current: [Row; 3] = [(1, "一"), (2, "二"), (3, "三")];
    assert_eq!(plan_rows(&current, &current, key), None, "一样时不写");
    assert_eq!(plan_rows(&current, &[(1, "一"), (2, "贰"), (3, "三")], key), plan(false, &[], &[1], false), "只改一行");
    assert_eq!(plan_rows(&current, &[(0, "零"), (1, "一"), (2, "二"), (3, "三")], key), plan(false, &[0], &[], false), "插在最前");
    assert_eq!(plan_rows(&current, &[(1, "一"), (4, "四"), (2, "二"), (3, "三"), (5, "五")], key), plan(false, &[1, 4], &[], false), "插在中间和末尾");
    assert_eq!(plan_rows(&current, &[(1, "一"), (3, "三")], key), plan(true, &[], &[], false), "删一行");
    assert_eq!(plan_rows(&current, &[(4, "四"), (1, "一"), (2, "二")], key), plan(true, &[0], &[], false), "新的进来、最旧的掉出去");
    assert_eq!(plan_rows(&current, &[(3, "三"), (2, "二"), (1, "一")], key), plan(false, &[], &[], true), "倒过来");
    assert_eq!(
        plan_rows(&current, &[(3, "叁"), (5, "五"), (1, "一")], key),
        plan(true, &[1], &[0], true),
        "删、插、改、排混在一起"
    );
    assert_eq!(plan_rows(&[], &current, key), plan(false, &[0, 1, 2], &[], false), "从空列表开始");
    assert_eq!(plan_rows(&current, &[], key), plan(true, &[], &[], false), "清空");
}

/// 现有列表里重复的键：只认第一行，删掉后面的；新列表里重复的键在写之前就只留第一行。
#[test]
fn duplicate_keys_keep_their_first_row() {
    let current: [Row; 3] = [(1, "一"), (1, "壹"), (2, "二")];
    assert_eq!(plan_rows(&current, &[(1, "一"), (2, "二")], key), plan(true, &[], &[], false), "现有列表的重复行要删");
    assert_eq!(plan_rows(&current, &[(1, "甲"), (2, "二")], key), plan(true, &[], &[0], false), "按第一行比内容");

    let list = store(current.to_vec());
    sync_rows(list, key, vec![(2, "二"), (1, "一"), (2, "重复"), (1, "又重复")]);
    assert_eq!(list.get_untracked(), [(2, "二"), (1, "一")], "两边的重复键都只留第一行");
}

/// 一串写入：每次写完的列表都和新列表（去掉重复键以后）一样。
#[test]
fn every_write_matches_the_new_list() {
    let list = store(vec![(1, "一"), (2, "二"), (3, "三"), (4, "四")]);
    let cases: [Vec<Row>; 7] = [
        vec![(1, "一"), (5, "五"), (2, "二"), (3, "三"), (4, "四")],
        vec![(1, "一"), (2, "贰"), (4, "四")],
        vec![(4, "四"), (2, "贰"), (1, "一"), (6, "六")],
        vec![(6, "六"), (6, "重复"), (1, "壹")],
        vec![(7, "七"), (1, "壹"), (8, "八"), (6, "陆")],
        vec![(8, "八"), (7, "七")],
        Vec::new(),
    ];
    for rows in cases {
        let mut expected = rows.clone();
        let mut seen = std::collections::HashSet::new();
        expected.retain(|row| seen.insert(row.0));
        sync_rows(list, key, rows);
        assert_eq!(list.get_untracked(), expected);
    }
}

/// 只读第 2 行的派生值：增、删、重排都不让它重算，改了第 2 行才重算一次。
#[test]
fn structural_writes_leave_kept_rows_alone() {
    let list = store(vec![(1, "一"), (2, "二"), (3, "三")]);
    let item = list.keyed(key).at(&2);
    let runs = Rc::new(Cell::new(0));
    let watched = {
        let runs = runs.clone();
        computed(move || {
            runs.set(runs.get() + 1);
            item.try_with(|row| row.1)
        })
    };
    assert_eq!(watched.get(), Some("二"));
    assert_eq!(runs.get(), 1);

    sync_rows(list, key, vec![(0, "零"), (3, "三"), (2, "二")]);
    assert_eq!(list.get_untracked(), [(0, "零"), (3, "三"), (2, "二")]);
    assert_eq!(watched.get(), Some("二"));
    assert_eq!(runs.get(), 1, "插、删、排不该让留下的行重算");

    sync_rows(list, key, vec![(0, "零"), (3, "叁"), (2, "二")]);
    assert_eq!(watched.get(), Some("二"));
    assert_eq!(runs.get(), 1, "改了别的行不该让这一行重算");

    sync_rows(list, key, vec![(0, "零"), (3, "叁"), (2, "贰")]);
    assert_eq!(watched.get(), Some("贰"));
    assert_eq!(runs.get(), 2, "改了这一行要重算一次");

    sync_rows(list, key, vec![(0, "零")]);
    assert_eq!(watched.get(), None, "删掉以后读到没有这一行");
}
