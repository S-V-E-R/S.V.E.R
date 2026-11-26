use super::*;

/// Channel tabs (`?tab=1`) skip the Home-only sections; Home still gets them.
pub async fn exercise(e: &Env) {
    e.user("AuditChanTabs", true).await;
    let home = e
        .anon
        .ok("GET", "/api/channels/AuditChanTabs", Value::Null)
        .await;
    assert!(home["war_council"]["members"].is_array());
    assert!(home["wall_preview"]["latest"].is_array());
    assert!(home["blocks"].is_array());
    let tab = e
        .anon
        .ok("GET", "/api/channels/AuditChanTabs?tab=1", Value::Null)
        .await;
    assert_eq!(tab["channel"]["username"], "AuditChanTabs");
    assert_eq!(tab["tabs"], home["tabs"]);
    assert_eq!(tab["header"], home["header"]);
    assert!(tab["war_council"].is_null());
    assert!(tab["wall_preview"].is_null());
    assert_eq!(tab["blocks"], json!([]));
}
