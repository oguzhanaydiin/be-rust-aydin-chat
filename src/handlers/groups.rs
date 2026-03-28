use actix_web::{web, HttpRequest, HttpResponse, Responder};
use futures::StreamExt;
use mongodb::bson::{doc, oid::ObjectId, to_bson, DateTime as BsonDateTime};

use crate::app_state::AppState;
use crate::auth::{verify_token, AuthClaims};
use crate::models::{
    AddGroupMemberRequest, ChatGroup, CreateGroupRequest, Friendship, FriendshipStatus,
    GroupDetailResponse, GroupMember, GroupMemberResponse, GroupRole, GroupSummaryResponse,
    UpdateGroupMemberPermissionsRequest, User,
};

fn verify_request_claims(data: &web::Data<AppState>, req: &HttpRequest) -> Result<AuthClaims, HttpResponse> {
    let auth_header = match req.headers().get("Authorization") {
        Some(header) => header,
        None => return Err(HttpResponse::Unauthorized().body("Missing Authorization header")),
    };

    let auth_str = match auth_header.to_str() {
        Ok(s) => s,
        Err(_) => return Err(HttpResponse::BadRequest().body("Invalid Authorization header")),
    };

    let token = if let Some(value) = auth_str.strip_prefix("Bearer ") {
        value
    } else {
        return Err(HttpResponse::BadRequest().body("Invalid Authorization format"));
    };

    verify_token(&data.jwt_secret, token)
        .map_err(|_| HttpResponse::Unauthorized().body("Invalid or expired token"))
}

fn normalize_identity(value: &str) -> String {
    value.trim().to_lowercase()
}

fn sorted_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_string(), right.to_string())
    } else {
        (right.to_string(), left.to_string())
    }
}

async fn resolve_username_by_email(
    db: &mongodb::Database,
    email: &str,
) -> Result<String, HttpResponse> {
    let users_col = db.collection::<User>("users");

    let found = users_col
        .find_one(doc! { "email": email }, None)
        .await
        .map_err(|e| {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        })?;

    let username = found.and_then(|u| u.username).unwrap_or_default();
    let normalized = normalize_identity(&username);

    if normalized.is_empty() {
        return Err(HttpResponse::BadRequest().body("username is not configured for this account"));
    }

    Ok(normalized)
}

async fn is_accepted_friend(
    db: &mongodb::Database,
    username_a: &str,
    username_b: &str,
) -> Result<bool, HttpResponse> {
    let (user_a, user_b) = sorted_pair(username_a, username_b);
    let friendships_col = db.collection::<Friendship>("friendships");

    let found = friendships_col
        .find_one(doc! { "user_a": &user_a, "user_b": &user_b }, None)
        .await
        .map_err(|e| {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        })?;

    Ok(matches!(found.map(|item| item.status), Some(FriendshipStatus::Accepted)))
}

async fn ensure_user_exists(db: &mongodb::Database, username: &str) -> Result<(), HttpResponse> {
    let users_col = db.collection::<User>("users");
    let found = users_col
        .find_one(doc! { "username": username }, None)
        .await
        .map_err(|e| {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        })?;

    if found.is_none() {
        return Err(HttpResponse::NotFound().body("target user not found"));
    }

    Ok(())
}

async fn get_group_member(
    db: &mongodb::Database,
    group_id: &str,
    username: &str,
) -> Result<Option<GroupMember>, HttpResponse> {
    let members_col = db.collection::<GroupMember>("group_members");
    members_col
        .find_one(doc! { "group_id": group_id, "username": username }, None)
        .await
        .map_err(|e| {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        })
}

async fn list_group_members(
    db: &mongodb::Database,
    group_id: &str,
) -> Result<Vec<GroupMember>, HttpResponse> {
    let members_col = db.collection::<GroupMember>("group_members");
    let mut cursor = members_col
        .find(doc! { "group_id": group_id }, None)
        .await
        .map_err(|e| {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        })?;

    let mut members = Vec::new();
    while let Some(item) = cursor.next().await {
        match item {
            Ok(member) => members.push(member),
            Err(e) => {
                return Err(HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "Database error",
                    "message": e.to_string()
                })))
            }
        }
    }

    members.sort_by(|a, b| a.username.cmp(&b.username));
    Ok(members)
}

pub async fn create_group(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateGroupRequest>,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let creator_username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };

    let name = body.name.trim().to_string();
    if name.is_empty() {
        return HttpResponse::BadRequest().body("group name is required");
    }
    if name.len() > 120 {
        return HttpResponse::BadRequest().body("group name is too long");
    }

    let mut initial_members = body
        .member_usernames
        .iter()
        .map(|item| normalize_identity(item))
        .filter(|item| !item.is_empty() && item != &creator_username)
        .collect::<Vec<String>>();
    initial_members.sort_unstable();
    initial_members.dedup();

    for candidate in &initial_members {
        if let Err(response) = ensure_user_exists(&data.db, candidate).await {
            return response;
        }

        match is_accepted_friend(&data.db, &creator_username, candidate).await {
            Ok(true) => {}
            Ok(false) => {
                return HttpResponse::BadRequest()
                    .body("you can only add users that are your accepted friends");
            }
            Err(response) => return response,
        }
    }

    let groups_col = data.db.collection::<ChatGroup>("chat_groups");
    let members_col = data.db.collection::<GroupMember>("group_members");

    let group_object_id = ObjectId::new();
    let group_id = group_object_id.to_hex();
    let now = BsonDateTime::now();

    let group_doc = ChatGroup {
        id: Some(group_object_id),
        name: name.clone(),
        created_by: creator_username.clone(),
        created_at: now,
        updated_at: now,
    };

    if let Err(e) = groups_col.insert_one(group_doc, None).await {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Database error",
            "message": e.to_string()
        }));
    }

    let leader_member = GroupMember {
        id: None,
        group_id: group_id.clone(),
        username: creator_username.clone(),
        role: GroupRole::Leader,
        can_invite: true,
        added_by: creator_username.clone(),
        created_at: now,
        updated_at: now,
    };

    if let Err(e) = members_col.insert_one(leader_member, None).await {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Database error",
            "message": e.to_string()
        }));
    }

    for member_username in &initial_members {
        let member = GroupMember {
            id: None,
            group_id: group_id.clone(),
            username: member_username.clone(),
            role: GroupRole::Member,
            can_invite: false,
            added_by: creator_username.clone(),
            created_at: now,
            updated_at: now,
        };

        if let Err(e) = members_col.insert_one(member, None).await {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }));
        }
    }

    let mut members = vec![GroupMemberResponse {
        username: creator_username.clone(),
        role: GroupRole::Leader,
        can_invite: true,
    }];
    for member_username in initial_members {
        members.push(GroupMemberResponse {
            username: member_username,
            role: GroupRole::Member,
            can_invite: false,
        });
    }

    HttpResponse::Created().json(GroupDetailResponse {
        group_id,
        name,
        created_by: creator_username,
        members,
    })
}

pub async fn list_groups(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let current_username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };

    let members_col = data.db.collection::<GroupMember>("group_members");
    let groups_col = data.db.collection::<ChatGroup>("chat_groups");

    let mut memberships_cursor = match members_col
        .find(doc! { "username": &current_username }, None)
        .await
    {
        Ok(cursor) => cursor,
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        }
    };

    let mut memberships: Vec<GroupMember> = Vec::new();
    while let Some(item) = memberships_cursor.next().await {
        match item {
            Ok(membership) => memberships.push(membership),
            Err(e) => {
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "Database error",
                    "message": e.to_string()
                }))
            }
        }
    }

    let mut response_items: Vec<GroupSummaryResponse> = Vec::new();

    for membership in memberships {
        let Ok(group_object_id) = ObjectId::parse_str(&membership.group_id) else {
            continue;
        };

        let group = match groups_col
            .find_one(doc! { "_id": group_object_id }, None)
            .await
        {
            Ok(Some(item)) => item,
            Ok(None) => continue,
            Err(e) => {
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "Database error",
                    "message": e.to_string()
                }))
            }
        };

        let member_count = match members_col
            .count_documents(doc! { "group_id": &membership.group_id }, None)
            .await
        {
            Ok(count) => count as usize,
            Err(e) => {
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "Database error",
                    "message": e.to_string()
                }))
            }
        };

        response_items.push(GroupSummaryResponse {
            group_id: membership.group_id,
            name: group.name,
            role: membership.role,
            can_invite: membership.can_invite,
            member_count,
        });
    }

    response_items.sort_by(|a, b| a.name.cmp(&b.name));
    HttpResponse::Ok().json(response_items)
}

pub async fn get_group_detail(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let current_username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };

    let group_id = path.into_inner();
    let normalized_group_id = group_id.trim().to_lowercase();
    if normalized_group_id.is_empty() {
        return HttpResponse::BadRequest().body("group_id is required");
    }

    match get_group_member(&data.db, &normalized_group_id, &current_username).await {
        Ok(Some(_)) => {}
        Ok(None) => return HttpResponse::Forbidden().body("you are not a member of this group"),
        Err(response) => return response,
    }

    let groups_col = data.db.collection::<ChatGroup>("chat_groups");
    let Ok(group_object_id) = ObjectId::parse_str(&normalized_group_id) else {
        return HttpResponse::BadRequest().body("invalid group_id");
    };

    let group = match groups_col
        .find_one(doc! { "_id": group_object_id }, None)
        .await
    {
        Ok(Some(item)) => item,
        Ok(None) => return HttpResponse::NotFound().body("group not found"),
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        }
    };

    let members = match list_group_members(&data.db, &normalized_group_id).await {
        Ok(items) => items,
        Err(response) => return response,
    };

    let member_items = members
        .into_iter()
        .map(|member| GroupMemberResponse {
            username: member.username,
            role: member.role,
            can_invite: member.can_invite,
        })
        .collect::<Vec<GroupMemberResponse>>();

    HttpResponse::Ok().json(GroupDetailResponse {
        group_id: normalized_group_id,
        name: group.name,
        created_by: group.created_by,
        members: member_items,
    })
}

pub async fn add_group_member(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<AddGroupMemberRequest>,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let inviter_username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };

    let group_id = normalize_identity(&path.into_inner());
    if group_id.is_empty() {
        return HttpResponse::BadRequest().body("group_id is required");
    }

    let inviter_member = match get_group_member(&data.db, &group_id, &inviter_username).await {
        Ok(Some(member)) => member,
        Ok(None) => return HttpResponse::Forbidden().body("you are not a member of this group"),
        Err(response) => return response,
    };

    let can_invite = inviter_member.role == GroupRole::Leader || inviter_member.can_invite;
    if !can_invite {
        return HttpResponse::Forbidden().body("you do not have permission to invite users");
    }

    let target_username = normalize_identity(&body.username);
    if target_username.is_empty() {
        return HttpResponse::BadRequest().body("username is required");
    }

    if target_username == inviter_username {
        return HttpResponse::BadRequest().body("cannot add yourself");
    }

    if let Err(response) = ensure_user_exists(&data.db, &target_username).await {
        return response;
    }

    match get_group_member(&data.db, &group_id, &target_username).await {
        Ok(Some(_)) => return HttpResponse::Conflict().body("user already in group"),
        Ok(None) => {}
        Err(response) => return response,
    }

    match is_accepted_friend(&data.db, &inviter_username, &target_username).await {
        Ok(true) => {}
        Ok(false) => {
            return HttpResponse::BadRequest().body("you can only add your accepted friends");
        }
        Err(response) => return response,
    }

    let members_col = data.db.collection::<GroupMember>("group_members");
    let now = BsonDateTime::now();
    let new_member = GroupMember {
        id: None,
        group_id: group_id.clone(),
        username: target_username,
        role: GroupRole::Member,
        can_invite: false,
        added_by: inviter_username,
        created_at: now,
        updated_at: now,
    };

    if let Err(e) = members_col.insert_one(new_member, None).await {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Database error",
            "message": e.to_string()
        }));
    }

    let groups_col = data.db.collection::<ChatGroup>("chat_groups");
    let Ok(group_object_id) = ObjectId::parse_str(&group_id) else {
        return HttpResponse::BadRequest().body("invalid group_id");
    };

    let _ = groups_col
        .update_one(
            doc! { "_id": group_object_id },
            doc! { "$set": { "updated_at": now } },
            None,
        )
        .await;

    HttpResponse::Created().json(serde_json::json!({ "ok": true }))
}

pub async fn update_group_member_permissions(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<(String, String)>,
    body: web::Json<UpdateGroupMemberPermissionsRequest>,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let actor_username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };

    let (raw_group_id, raw_target_username) = path.into_inner();
    let group_id = normalize_identity(&raw_group_id);
    let target_username = normalize_identity(&raw_target_username);

    if group_id.is_empty() || target_username.is_empty() {
        return HttpResponse::BadRequest().body("group_id and username are required");
    }

    let actor_member = match get_group_member(&data.db, &group_id, &actor_username).await {
        Ok(Some(member)) => member,
        Ok(None) => return HttpResponse::Forbidden().body("you are not a member of this group"),
        Err(response) => return response,
    };

    if actor_member.role != GroupRole::Leader {
        return HttpResponse::Forbidden().body("only group leaders can manage permissions");
    }

    let target_member = match get_group_member(&data.db, &group_id, &target_username).await {
        Ok(Some(member)) => member,
        Ok(None) => return HttpResponse::NotFound().body("target member not found"),
        Err(response) => return response,
    };

    let desired_role = body.role.clone().unwrap_or_else(|| target_member.role.clone());
    let mut desired_can_invite = body.can_invite.unwrap_or(target_member.can_invite);

    if desired_role == GroupRole::Leader {
        desired_can_invite = true;
    }

    if target_member.role == GroupRole::Leader
        && desired_role == GroupRole::Leader
        && body.can_invite == Some(false)
    {
        return HttpResponse::BadRequest().body("leader must keep invite permission");
    }

    let members_col = data.db.collection::<GroupMember>("group_members");
    let now = BsonDateTime::now();

    let role_bson = match to_bson(&desired_role) {
        Ok(value) => value,
        Err(_) => return HttpResponse::InternalServerError().body("failed to serialize role"),
    };

    if let Err(e) = members_col
        .update_one(
            doc! { "group_id": &group_id, "username": &target_username },
            doc! { "$set": { "role": role_bson, "can_invite": desired_can_invite, "updated_at": now } },
            None,
        )
        .await
    {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Database error",
            "message": e.to_string()
        }));
    }

    let groups_col = data.db.collection::<ChatGroup>("chat_groups");
    if let Ok(group_object_id) = ObjectId::parse_str(&group_id) {
        let _ = groups_col
            .update_one(
                doc! { "_id": group_object_id },
                doc! { "$set": { "updated_at": now } },
                None,
            )
            .await;
    }

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

pub async fn remove_group_member(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<(String, String)>,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let actor_username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };

    let (raw_group_id, raw_target_username) = path.into_inner();
    let group_id = normalize_identity(&raw_group_id);
    let target_username = normalize_identity(&raw_target_username);

    if group_id.is_empty() || target_username.is_empty() {
        return HttpResponse::BadRequest().body("group_id and username are required");
    }

    let actor_member = match get_group_member(&data.db, &group_id, &actor_username).await {
        Ok(Some(member)) => member,
        Ok(None) => return HttpResponse::Forbidden().body("you are not a member of this group"),
        Err(response) => return response,
    };

    if actor_member.role != GroupRole::Leader {
        return HttpResponse::Forbidden().body("only group leaders can remove members");
    }

    if actor_username == target_username {
        return HttpResponse::BadRequest().body("leader cannot remove self from this endpoint");
    }

    let target_member = match get_group_member(&data.db, &group_id, &target_username).await {
        Ok(Some(member)) => member,
        Ok(None) => return HttpResponse::NotFound().body("target member not found"),
        Err(response) => return response,
    };

    if target_member.role == GroupRole::Leader {
        let members = match list_group_members(&data.db, &group_id).await {
            Ok(items) => items,
            Err(response) => return response,
        };

        let leader_count = members
            .iter()
            .filter(|member| member.role == GroupRole::Leader)
            .count();

        if leader_count <= 1 {
            return HttpResponse::BadRequest().body("group must have at least one leader");
        }
    }

    let members_col = data.db.collection::<GroupMember>("group_members");
    if let Err(e) = members_col
        .delete_one(doc! { "group_id": &group_id, "username": &target_username }, None)
        .await
    {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Database error",
            "message": e.to_string()
        }));
    }

    let groups_col = data.db.collection::<ChatGroup>("chat_groups");
    if let Ok(group_object_id) = ObjectId::parse_str(&group_id) {
        let _ = groups_col
            .update_one(
                doc! { "_id": group_object_id },
                doc! { "$set": { "updated_at": BsonDateTime::now() } },
                None,
            )
            .await;
    }

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}
