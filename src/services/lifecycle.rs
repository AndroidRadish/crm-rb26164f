//! 客户全生命周期状态机（RB-266FB3 §2.1）
//!
//! 状态载体：customers.stage → stages 表（tenant 自定义阶段）。
//! 合法流转规则（后端强制，不只前端拦）：
//! 1. 初始放置（from = NULL）：可放入任何 is_active 阶段（含终态，用于导入/新建直录）
//! 2. 非终态 → 相邻非终态阶段：可前进一步或后退一步（看板拖拽兼容：拖错可拖回）
//! 3. 非终态 → 终态（is_won / is_lost）：任意时刻可判赢单/输单
//! 4. 终态 → 任何阶段：拒绝（赢单/输单为终态，不可再流转）
//! 5. 目标阶段已停用（is_active = false）：拒绝
//! 6. from = 已停用的非终态阶段（历史数据）：允许迁出到任意活性阶段（数据修复路径）
//!
//! 阶段不存在 / 跨租户：调用方先校验，本函数假定 stages 为同 tenant 全集。

use uuid::Uuid;

use crate::models::stage::Stage;

/// 校验客户阶段流转。stages 为该 tenant 全部阶段（含已停用）。
/// 返回 Err(中文原因) 表示非法流转，调用方转 409。
pub fn validate_transition(stages: &[Stage], from: Option<Uuid>, to: Uuid) -> Result<(), String> {
    // 目标阶段必须存在
    let to_stage = stages
        .iter()
        .find(|s| s.id == to)
        .ok_or_else(|| "目标阶段不存在".to_string())?;

    // 规则 5：停用阶段不可作为目标
    if !to_stage.is_active {
        return Err(format!("目标阶段「{}」已停用", to_stage.name));
    }

    let from_id = match from {
        None => return Ok(()), // 规则 1：初始放置
        Some(f) => f,
    };

    if from_id == to {
        return Ok(()); // 原地放置，无流转
    }

    // from 阶段在 stages 里找不到视为脏数据：放行到活性阶段（同数据修复路径）
    let Some(from_stage) = stages.iter().find(|s| s.id == from_id) else {
        return Ok(());
    };

    // 规则 4：终态不可再流转
    if from_stage.is_won || from_stage.is_lost {
        return Err(format!(
            "「{}」是终态（{}），不可再流转",
            from_stage.name,
            if from_stage.is_won { "赢单" } else { "输单" }
        ));
    }

    // 规则 3：非终态 → 终态，任意时刻合法
    if to_stage.is_won || to_stage.is_lost {
        return Ok(());
    }

    // 规则 6：from 已停用（历史数据），允许迁出到任意活性阶段
    if !from_stage.is_active {
        return Ok(());
    }

    // 规则 2：非终态活性链上只能相邻移动（前进一步或后退一步）
    let mut chain: Vec<&Stage> = stages
        .iter()
        .filter(|s| s.is_active && !s.is_won && !s.is_lost)
        .collect();
    chain.sort_by_key(|s| s.order_index);

    let from_pos = chain.iter().position(|s| s.id == from_id);
    let to_pos = chain.iter().position(|s| s.id == to);
    match (from_pos, to_pos) {
        (Some(f), Some(t)) if f.abs_diff(t) == 1 => Ok(()),
        _ => Err(format!(
            "非法流转：「{}」→「{}」跨阶段跳跃；只能移动到相邻阶段，或直接判定赢单/输单",
            from_stage.name, to_stage.name
        )),
    }
}

/// 从 DB 载入 tenant 全部阶段（含停用），供流转校验
pub async fn load_stages(pool: &sqlx::PgPool, tenant_id: Uuid) -> Result<Vec<Stage>, sqlx::Error> {
    sqlx::query_as::<_, Stage>(
        "SELECT id, tenant_id, name, order_index, is_won, is_lost, is_active, created_at \
         FROM stages WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn stage(id: Uuid, name: &str, order: i32, won: bool, lost: bool, active: bool) -> Stage {
        Stage {
            id,
            tenant_id: Uuid::new_v4(),
            name: name.to_string(),
            order_index: order,
            is_won: won,
            is_lost: lost,
            is_active: active,
            created_at: Utc::now(),
        }
    }

    fn fixture() -> (Vec<Stage>, Uuid, Uuid, Uuid, Uuid, Uuid) {
        let lead = Uuid::new_v4();
        let intent = Uuid::new_v4();
        let quote = Uuid::new_v4();
        let won = Uuid::new_v4();
        let lost = Uuid::new_v4();
        let stages = vec![
            stage(lead, "线索", 0, false, false, true),
            stage(intent, "意向", 1, false, false, true),
            stage(quote, "报价", 2, false, false, true),
            stage(won, "赢单", 3, true, false, true),
            stage(lost, "输单", 4, false, true, true),
        ];
        (stages, lead, intent, quote, won, lost)
    }

    #[test]
    fn initial_placement_any_active_stage() {
        let (stages, lead, _, _, won, _) = fixture();
        assert!(validate_transition(&stages, None, lead).is_ok());
        assert!(validate_transition(&stages, None, won).is_ok());
    }

    #[test]
    fn adjacent_forward_and_backward_ok() {
        let (stages, lead, intent, quote, _, _) = fixture();
        assert!(validate_transition(&stages, Some(lead), intent).is_ok());
        assert!(validate_transition(&stages, Some(intent), lead).is_ok());
        assert!(validate_transition(&stages, Some(intent), quote).is_ok());
    }

    #[test]
    fn jump_rejected() {
        let (stages, lead, _, quote, _, _) = fixture();
        assert!(validate_transition(&stages, Some(lead), quote).is_err());
    }

    #[test]
    fn terminal_rules() {
        let (stages, lead, intent, _, won, lost) = fixture();
        // 非终态 → 终态：合法
        assert!(validate_transition(&stages, Some(lead), won).is_ok());
        assert!(validate_transition(&stages, Some(intent), lost).is_ok());
        // 终态 → 任何：拒绝
        assert!(validate_transition(&stages, Some(won), lead).is_err());
        assert!(validate_transition(&stages, Some(won), lost).is_err());
        assert!(validate_transition(&stages, Some(lost), intent).is_err());
    }

    #[test]
    fn inactive_target_rejected() {
        let (mut stages, lead, intent, _, _, _) = fixture();
        stages[1].is_active = false; // 意向停用
        assert!(validate_transition(&stages, Some(lead), intent).is_err());
        assert!(validate_transition(&stages, None, intent).is_err());
    }

    #[test]
    fn same_stage_noop_ok() {
        let (stages, lead, _, _, _, _) = fixture();
        assert!(validate_transition(&stages, Some(lead), lead).is_ok());
    }

    #[test]
    fn inactive_source_can_move_out() {
        let (mut stages, lead, intent, quote, _, _) = fixture();
        stages[0].is_active = false; // 线索停用（历史数据）
        assert!(validate_transition(&stages, Some(lead), intent).is_ok());
        assert!(validate_transition(&stages, Some(lead), quote).is_ok());
    }
}
