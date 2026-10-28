//! Messages, the server's half (operator, 2026-10-18: "as an admin, be able
//! to look at the requests, who sent the request, and then be able to send
//! them a message back ... the ability to chat with other users that are
//! currently live and who are interested or willing ... public or private,
//! default to private ... each user needs to be able to send a request
//! easily ... resolve it, and then also them get a message within the same
//! location ... with the outcome").
//!
//! Three tables (src/sql/tables): requests are `complaints` rows with their
//! thread in `request_messages`; `user_presence` is who is here and whether
//! they are public; `direct_messages` is person to person. Every answer is
//! for `owner`, the username the session was signed in as - never a name the
//! page sends - and every value reaches SQL as a psql variable (`:'x'`),
//! quoted by psql itself.
//!
//! An admin's answer or status change goes through the platform's own
//! complaints$update_complaint, so the request's site thread and the older
//! complaints.ndexr.io board stay the same story.

use serde_json::Value;

use crate::api::{InboxAct, InboxView};

/// Who `owner` is, as every query starts: their email, username and whether
/// they are an admin. Empty when the name is no ndexr account.
const ME: &str = "me as (select lower(u.email) as email, u.username, \
       coalesce((select a.is_admin from admin_users a where lower(a.email) = lower(u.email) limit 1), false) as admin \
     from ndexr_users u where lower(u.username) = lower(:'owner') limit 1)";

/// A person as others see them: their username, or (to an admin) their email.
/// Email domains that are not an organization: anyone can have an address
/// there, so "people with my email's domain" would be strangers.
const ORG_EXCLUDED: &str = "'gmail.com', 'googlemail.com', 'outlook.com', 'hotmail.com', 'live.com', 'msn.com', 'yahoo.com', \
     'ymail.com', 'icloud.com', 'me.com', 'mac.com', 'aol.com', 'proton.me', 'protonmail.com', 'pm.me', 'gmx.com', 'gmx.net', \
     'mail.com', 'yandex.com', 'zoho.com', 'fastmail.com', 'hey.com', 'tutanota.com', 'qq.com', '163.com', 'example.com'";

/// SQL: may the person in user_presence row `p` be found by `me` (an email
/// expression)? Their own choice decides (user_presence.findable): anyone,
/// or people with their email's domain when that domain is an organization's.
fn reach(p: &str, me: &str) -> String {
    format!(
        "({p}.findable = 'public' or ({p}.findable = 'org' and split_part({p}.email, '@', 2) = split_part({me}, '@', 2) \
           and split_part({p}.email, '@', 2) not in ({ORG_EXCLUDED})))"
    )
}

const NAME_OF: &str = "coalesce((select nu.username from ndexr_users nu where lower(nu.email) = lower(EMAIL) and nu.username <> '' limit 1), \
       case when (select admin from me) then EMAIL else 'someone' end)";

fn name_of(col: &str) -> String {
    NAME_OF.replace("EMAIL", col)
}

fn json(sql: &str, vars: &[(&str, &str)]) -> Result<Value, String> {
    let out = crate::host::platform_psql_vars(sql, vars)?;
    let s = String::from_utf8_lossy(&out);
    serde_json::from_str(s.trim()).map_err(|e| format!("messages: {e}"))
}

/// Everything Messages shows `owner`, and a stamp that they are here.
pub fn inbox(owner: &str, all: bool, chat: &str, find: &str) -> Result<InboxView, String> {
    let who = name_of("r.email");
    let author = name_of("m.author_email");
    // An admin's message is the team's, even on a request they sent themselves,
    // so an admin asking themselves sees the answer arrive as anyone would.
    let staff = "exists (select 1 from admin_users a where lower(a.email) = lower(m.author_email) and a.is_admin)";
    let found_by_me = reach("p", "(select email from me)");
    let sql = format!(
        "with {ME}, \
         stamp as (insert into user_presence (email, username, last_seen) select email, username, now() from me \
                   on conflict (email) do update set last_seen = now(), username = excluded.username returning public, findable), \
         them as (select lower(u.email) as email from ndexr_users u where lower(u.username) = lower(:'chat') and :'chat' <> '' limit 1), \
         readit as (update direct_messages set read_at = now() \
                    where lower(to_email) = (select email from me) and lower(from_email) = (select email from them) and read_at is null returning id), \
         reqs as (select c.* from complaints c \
                  where lower(c.email) = (select email from me) or ((select admin from me) and :'all' = '1') \
                  order by c.created_at desc limit 150), \
         talked as (select distinct case when lower(from_email) = (select email from me) then lower(to_email) else lower(from_email) end as email \
                    from direct_messages where to_group is null and (lower(from_email) = (select email from me) or lower(to_email) = (select email from me))), \
         conn as (select c.*, case when c.from_email = (select email from me) then c.to_email else c.from_email end as other \
                  from connections c where (select email from me) in (c.from_email, c.to_email) and c.status <> 'declined'), \
         folk as (select p.email, p.username, p.public, p.last_seen > now() - interval '2 minutes' as live from user_presence p \
                  where p.email <> (select email from me) and p.username <> '' \
                    and (({found_by_me} and p.last_seen > now() - interval '2 minutes') or p.email in (select email from talked) \
                         or p.email in (select other from conn where status = 'accepted'))), \
         mine_g as (select g.*, m.status as my from people_groups g \
                    join people_group_members m on m.group_id = g.id and m.email = (select email from me)), \
         grp as (select id from mine_g where my = 'member' and :'chat' = '#' || id::text) \
         select json_build_object( \
           'me', coalesce((select username from me), ''), \
           'admin', coalesce((select admin from me), false), \
           'public', coalesce((select public from stamp limit 1), false), \
           'findable', coalesce((select findable from stamp limit 1), 'private'), \
           'has_org', coalesce((select split_part(email, '@', 2) not in ({ORG_EXCLUDED}) from me), false), \
           'open', case when (select admin from me) then (select count(*) from complaints where status in ('open', 'working')) else 0 end, \
           'connections', coalesce((select json_agg(json_build_object( \
               'username', {other}, 'status', c.status, \
               'incoming', c.status = 'pending' and c.to_email = (select email from me), \
               'live', exists (select 1 from user_presence p where p.email = c.other and p.last_seen > now() - interval '2 minutes')) \
             order by c.status desc, c.created_at desc) from conn c), '[]'::json), \
           'groups', coalesce((select json_agg(json_build_object( \
               'id', g.id, 'name', g.name, 'mine', g.created_by = (select email from me), 'member', g.my = 'member', \
               'members', coalesce((select json_agg({member} order by m.created_at) from people_group_members m where m.group_id = g.id and m.status = 'member'), '[]'::json), \
               'invited', coalesce((select json_agg({member} order by m.created_at) from people_group_members m where m.group_id = g.id and m.status = 'invited'), '[]'::json), \
               'last', coalesce(l.last, ''), 'last_at', coalesce(to_char(l.at at time zone 'UTC', 'YYYY-MM-DD HH24:MI'), '')) \
             order by l.at desc nulls last, g.name) \
             from mine_g g left join lateral ( \
               select case d.kind when 'file' then '📎 ' || d.body when 'site' then '🌐 ' || d.body else d.body end as last, d.created_at as at \
               from direct_messages d where d.to_group = g.id and g.my = 'member' order by d.id desc limit 1) l on true), '[]'::json), \
           'shares', coalesce((select json_agg(json_build_object( \
               'id', s.id, 'domain', s.domain, 'role', s.role, 'status', s.status, 'owner', {owner_name}, \
               'mine', s.owner_email = (select email from me), \
               'to', coalesce((select g.name from people_groups g where g.id = s.to_group), {to_name}), \
               'group', coalesce(s.to_group, 0), \
               'at', to_char(s.created_at at time zone 'UTC', 'YYYY-MM-DD HH24:MI')) order by s.created_at desc) \
             from shares s \
             where s.owner_email = (select email from me) \
                or (s.to_email = (select email from me) and s.status <> 'declined') \
                or (s.status = 'accepted' and s.to_group in (select id from mine_g where my = 'member'))), '[]'::json), \
           'requests', coalesce((select json_agg(json_build_object( \
               'id', r.id, 'username', {who}, \
               'email', case when (select admin from me) then r.email else '' end, \
               'domain', r.domain, 'what', r.what_sucks, 'trying', coalesce(r.trying_to, ''), \
               'status', r.status, 'urgency', r.urgency, \
               'created', to_char(r.created_at at time zone 'UTC', 'YYYY-MM-DD HH24:MI'), \
               'n_uploads', r.n_uploads, \
               'mine', lower(r.email) = (select email from me), \
               'unread', case when lower(r.email) = (select email from me) \
                   then exists (select 1 from request_messages m where m.complaint_id = r.id \
                                and (lower(m.author_email) <> lower(r.email) or {staff}) and m.created_at > coalesce(r.thread_seen_at, 'epoch')) \
                   else (select admin from me) and r.status in ('open', 'working') and coalesce((select lower(m.author_email) = lower(r.email) \
                         from request_messages m where m.complaint_id = r.id order by m.created_at desc limit 1), true) end, \
               'messages', coalesce((select json_agg(json_build_object( \
                   'who', {author}, 'kind', m.kind, 'body', m.body, \
                   'at', to_char(m.created_at at time zone 'UTC', 'YYYY-MM-DD HH24:MI'), \
                   'mine', lower(m.author_email) = (select email from me), \
                   'team', lower(m.author_email) <> lower(r.email) or {staff}) order by m.created_at) \
                 from request_messages m where m.complaint_id = r.id), '[]'::json)) \
             order by r.created_at desc) from reqs r), '[]'::json), \
           'people', coalesce((select json_agg(json_build_object( \
               'username', f.username, 'live', f.live, 'public', f.public, \
               'unread', (select count(*) from direct_messages d where lower(d.from_email) = f.email \
                          and lower(d.to_email) = (select email from me) and d.read_at is null), \
               'last', coalesce(f.last, ''), 'last_at', coalesce(to_char(f.last_at at time zone 'UTC', 'YYYY-MM-DD HH24:MI'), ''), \
               'admin', exists (select 1 from admin_users a where lower(a.email) = f.email and a.is_admin)) \
             order by f.last_at desc nulls last, f.live desc, f.username) \
             from (select f.*, l.last, l.last_at from folk f left join lateral ( \
                     select case coalesce(to_jsonb(d)->>'kind', 'text') when 'file' then '📎 ' || d.body when 'site' then '🌐 ' || d.body else d.body end as last, \
                            d.created_at as last_at \
                     from direct_messages d \
                     where (lower(d.from_email) = (select email from me) and lower(d.to_email) = f.email) \
                        or (lower(d.to_email) = (select email from me) and lower(d.from_email) = f.email) \
                     order by d.id desc limit 1) l on true) f), '[]'::json), \
           'found', coalesce((select json_agg(json_build_object('username', f.username, 'live', f.live, 'public', true, 'unread', 0, \
               'admin', exists (select 1 from admin_users a where lower(a.email) = f.email and a.is_admin)) \
               order by f.exact desc, f.live desc, f.username) \
             from (select p.email, p.username, p.last_seen > now() - interval '2 minutes' as live, lower(p.username) = lower(:'find') as exact \
                   from user_presence p where {found_by_me} and p.username <> '' and p.email <> (select email from me) \
                     and :'find' <> '' and position(lower(:'find') in lower(p.username)) > 0 \
                   order by lower(p.username) = lower(:'find') desc, p.last_seen desc limit 12) f), '[]'::json), \
           'chat', coalesce((select json_agg(x order by x.id) from (select d.id, \
               lower(d.from_email) = (select email from me) as mine, d.body, d.kind, coalesce(d.size, 0) as size, \
               d.read_at is not null as read, {sender} as who, \
               to_char(d.created_at at time zone 'UTC', 'YYYY-MM-DD HH24:MI') as at \
             from direct_messages d \
             where (d.to_group is null and exists (select 1 from them) \
                    and ((lower(d.from_email) = (select email from me) and lower(d.to_email) = (select email from them)) \
                      or (lower(d.to_email) = (select email from me) and lower(d.from_email) = (select email from them)))) \
                or d.to_group = (select id from grp) \
             order by d.id desc limit 200) x), '[]'::json))",
        other = name_of("c.other"),
        member = name_of("m.email"),
        owner_name = name_of("s.owner_email"),
        to_name = name_of("s.to_email"),
        sender = name_of("d.from_email"),
    );
    let v = json(&sql, &[("owner", owner), ("all", if all { "1" } else { "0" }), ("chat", chat.trim()), ("find", find.trim())])?;
    serde_json::from_value(v).map_err(|e| format!("messages: {e}"))
}

/// The platform's update_complaint, as an admin: the reply on the thread
/// and in the site's thread, the status with who moved it.
const UPDATE_R: &str = r#"
args <- commandArgs(TRUE)
id <- as.integer(args[[1]]); status <- args[[2]]; by <- args[[4]]
reply <- paste(readLines(args[[3]], warn = FALSE), collapse = "\n")
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(cp = r/complaints/complaints)
r <- tryCatch({
  cp$update_complaint(id, status = if (nzchar(status)) status else NULL,
                      reply = if (nzchar(reply)) reply else NULL, by = by)
  list(ok = TRUE)
}, error = function(e) list(error = conditionMessage(e)))
cat(jsonlite::toJSON(r, auto_unbox = TRUE))
"#;

const STATUSES: [&str; 4] = ["open", "working", "resolved", "wontfix"];

/// Do `act` as `owner`, after checking they may.
pub fn act(owner: &str, act: &InboxAct) -> Result<Value, String> {
    // Who they are, and for a request whose it is.
    let rid = match act {
        InboxAct::Post { id, .. } | InboxAct::Status { id, .. } | InboxAct::Seen { id } => *id,
        _ => 0,
    };
    let rid_s = rid.to_string();
    let who = json(
        &format!(
            "with {ME} select json_build_object('email', coalesce((select email from me), ''), 'admin', coalesce((select admin from me), false), \
             'sender', coalesce((select lower(email) from complaints where id = :'rid'::bigint), ''), \
             'status', coalesce((select status from complaints where id = :'rid'::bigint), ''))"
        ),
        &[("owner", owner), ("rid", &rid_s)],
    )?;
    let email = who["email"].as_str().unwrap_or_default().to_owned();
    if email.is_empty() {
        return Err(format!("no ndexr account named {owner}"));
    }
    let admin = who["admin"].as_bool().unwrap_or(false);
    let mine = who["sender"].as_str() == Some(email.as_str());
    let clip = |b: &str| -> String { b.trim().chars().take(4000).collect() };
    match act {
        InboxAct::Post { id, body } => {
            let body = clip(body);
            if body.is_empty() {
                return Err("write something first".into());
            }
            if admin {
                // The team answering - on their own request too: the platform's own
                // path, which also moves an open request to working.
                let status = if who["status"].as_str() == Some("open") { "working" } else { "" };
                return update_as(owner, *id, status, &body, &email);
            }
            if !mine {
                return Err("only its sender or an admin writes on a request".into());
            }
            json(
                // A sender writing on a closed request has more to say: it opens again.
                "insert into request_messages (complaint_id, author_email, kind, body) values (:'rid'::bigint, :'email', 'message', :'body'); \
                 update complaints set thread_seen_at = now() where id = :'rid'::bigint; \
                 with reopened as (update complaints set status = 'open', resolved_at = null, resolved_by = null \
                   where id = :'rid'::bigint and status in ('resolved', 'wontfix') returning id) \
                 insert into request_messages (complaint_id, author_email, kind, body) select id, :'email', 'status', 'open' from reopened; \
                 select '{\"ok\":true}'",
                &[("rid", &rid_s), ("email", &email), ("body", &body)],
            )
        }
        InboxAct::Status { id, status } => {
            if !STATUSES.contains(&status.as_str()) {
                return Err(format!("{status}: not a status"));
            }
            if !admin && (!mine || !matches!(status.as_str(), "resolved" | "open")) {
                return Err("a request's sender can mark it resolved, or open it again".into());
            }
            // Already so: nothing to change and nothing to add to the thread
            // (#33 got "marked it resolved" twice, eight seconds apart).
            if who["status"].as_str() == Some(status.as_str()) {
                return Ok(serde_json::json!({ "ok": true, "unchanged": true }));
            }
            if admin {
                return update_as(owner, *id, status, "", &email);
            }
            json(
                "update complaints set status = :'status', \
                   resolved_at = case when :'status' = 'resolved' then now() else null end, \
                   resolved_by = case when :'status' = 'resolved' then :'email' else null end \
                 where id = :'rid'::bigint; \
                 insert into request_messages (complaint_id, author_email, kind, body) values (:'rid'::bigint, :'email', 'status', :'status'); \
                 select '{\"ok\":true}'",
                &[("rid", &rid_s), ("email", &email), ("status", status)],
            )
        }
        InboxAct::Seen { .. } => json(
            "update complaints set thread_seen_at = now() where id = :'rid'::bigint and lower(email) = :'email'; select '{\"ok\":true}'",
            &[("rid", &rid_s), ("email", &email)],
        ),
        InboxAct::Public { on } => set_findable(&email, if *on { "public" } else { "private" }),
        InboxAct::Findable { level } => set_findable(&email, level),
        InboxAct::Dm { to, body, kind, file } => {
            let kind = if kind.is_empty() { "text" } else { kind.as_str() };
            let bytes = match (kind, file) {
                ("text" | "site", None) => None,
                ("file", Some(f)) => {
                    let b = f.bytes().ok_or("that file did not arrive whole - send it again")?;
                    if b.len() > crate::api::DM_FILE_MAX {
                        return Err(format!("files in Chat are up to {} MB", crate::api::DM_FILE_MAX >> 20));
                    }
                    Some(b)
                }
                _ => return Err(format!("{kind}: not something Chat sends")),
            };
            let body = match (kind, file) {
                ("file", Some(f)) => file_name(&f.name).ok_or("that file has no usable name")?,
                ("site", _) => {
                    let h = body.trim().to_lowercase();
                    if !host_ok(&h) {
                        return Err(format!("{h}: not a site's address"));
                    }
                    h
                }
                _ => clip(body),
            };
            if body.is_empty() {
                return Err("write something first".into());
            }
            // A group's chat: its members write in it.
            let (target, group) = if let Some(g) = to.trim().strip_prefix('#') {
                let gid: i64 = g.parse().map_err(|_| format!("{to}: not a group"))?;
                let g = gid.to_string();
                let v = json(
                    "select json_build_object('ok', exists (select 1 from people_group_members where group_id = :'g'::bigint and email = :'email' and status = 'member'))",
                    &[("g", &g), ("email", &email)],
                )?;
                if !v["ok"].as_bool().unwrap_or(false) {
                    return Err("you are not in that group".into());
                }
                (String::new(), g)
            } else {
                // To someone public, connected with me, or who has written to
                // me: a private person is reached only by those they let in.
                // An admin reaches anyone.
                let v = json(
                    &format!("with them as (select lower(u.email) as email from ndexr_users u where lower(u.username) = lower(:'to') limit 1) \
                     select json_build_object('to', coalesce((select email from them), ''), \
                       'ok', :'admin' = '1' \
                          or exists (select 1 from user_presence p, them where p.email = them.email and {reach}) \
                          or exists (select 1 from direct_messages d, them where lower(d.from_email) = them.email and lower(d.to_email) = :'email') \
                          or exists (select 1 from connections c, them where c.status = 'accepted' \
                                     and least(c.from_email, c.to_email) = least(them.email, :'email') \
                                     and greatest(c.from_email, c.to_email) = greatest(them.email, :'email')))", reach = reach("p", ":'email'")),
                    &[("to", to.trim()), ("email", &email), ("admin", if admin { "1" } else { "0" })],
                )?;
                let target = v["to"].as_str().unwrap_or_default().to_owned();
                if target.is_empty() {
                    return Err(format!("no ndexr account named {to}"));
                }
                if target == email {
                    return Err("that is you".into());
                }
                if !v["ok"].as_bool().unwrap_or(false) {
                    return Err(format!("{to} is private - ask to connect, and they can accept"));
                }
                (target, String::new())
            };
            let size = bytes.as_ref().map_or(0, Vec::len).to_string();
            let v = json(
                "with m as (insert into direct_messages (from_email, to_email, body, kind, size, to_group) \
                   values (:'email', :'to', :'body', :'kind', nullif(:'size', '0')::bigint, nullif(:'group', '')::bigint) returning id) \
                 select json_build_object('id', id) from m",
                &[("email", &email), ("to", &target), ("body", &body), ("kind", kind), ("size", &size), ("group", &group)],
            )?;
            if let Some(b) = bytes {
                let id = v["id"].as_i64().ok_or("the message was not stored")?;
                let dir = files_dir()?;
                if let Err(e) = std::fs::write(dir.join(id.to_string()), &b) {
                    let _ = json("delete from direct_messages where id = :'id'::bigint; select '{}'", &[("id", &id.to_string())]);
                    return Err(format!("the file was not stored: {e}"));
                }
            }
            Ok(serde_json::json!({ "ok": true }))
        }
        InboxAct::Connect { to } => {
            let other = email_of(to)?;
            if other == email {
                return Err("that is you".into());
            }
            // One row a pair. Asking someone who asked me accepts theirs; asking
            // again after I turned them down asks them. Asking again after they
            // turned me down changes nothing: no one is asked twice.
            let v = checked(
                "with c as (insert into connections (from_email, to_email) values (:'email', :'other') \
                   on conflict (least(from_email, to_email), greatest(from_email, to_email)) do update set \
                     status = case when connections.status = 'pending' and connections.from_email = :'other' then 'accepted' \
                                   when connections.status = 'declined' and connections.to_email = :'email' then 'pending' \
                                   else connections.status end, \
                     from_email = case when connections.status = 'declined' and connections.to_email = :'email' then :'email' else connections.from_email end, \
                     to_email = case when connections.status = 'declined' and connections.to_email = :'email' then :'other' else connections.to_email end, \
                     decided_at = case when connections.status = 'pending' and connections.from_email = :'other' then now() else connections.decided_at end \
                   returning status) \
                 select json_build_object('status', (select status from c))",
                &[("email", &email), ("other", &other)],
            )?;
            Ok(serde_json::json!({ "ok": true, "status": v["status"] }))
        }
        InboxAct::Connection { with, accept } => {
            let other = email_of(with)?;
            let sql = if *accept {
                "update connections set status = 'accepted', decided_at = now() \
                   where from_email = :'other' and to_email = :'email' and status = 'pending'; select '{\"ok\":true}'"
            } else {
                // Theirs to me: declined. Mine still waiting, or an accepted
                // one either way: gone.
                "update connections set status = 'declined', decided_at = now() \
                   where from_email = :'other' and to_email = :'email' and status = 'pending'; \
                 delete from connections where least(from_email, to_email) = least(:'other', :'email') \
                   and greatest(from_email, to_email) = greatest(:'other', :'email') \
                   and (status = 'accepted' or (status = 'pending' and from_email = :'email')); select '{\"ok\":true}'"
            };
            json(sql, &[("email", &email), ("other", &other)])
        }
        InboxAct::GroupNew { name } => {
            let name: String = name.trim().chars().take(60).collect();
            if name.is_empty() {
                return Err("give the group a name".into());
            }
            json(
                "with g as (insert into people_groups (name, created_by) values (:'name', :'email') returning id), \
                      m as (insert into people_group_members (group_id, email, status, invited_by) select id, :'email', 'member', :'email' from g) \
                 select json_build_object('ok', true, 'id', (select id from g))",
                &[("name", &name), ("email", &email)],
            )
        }
        InboxAct::GroupInvite { id, who } => {
            let other = email_of(who)?;
            // Into a group I am in, someone I am connected with.
            checked(
                "select case \
                   when not exists (select 1 from people_group_members where group_id = :'g'::bigint and email = :'email' and status = 'member') \
                     then json_build_object('error', 'you are not in that group') \
                   when :'admin' <> '1' and not exists (select 1 from connections where status = 'accepted' \
                          and least(from_email, to_email) = least(:'other', :'email') and greatest(from_email, to_email) = greatest(:'other', :'email')) \
                     then json_build_object('error', 'connect with them first - then you can invite them') \
                   else json_build_object('ok', true) end",
                &[("g", &id.to_string()), ("email", &email), ("other", &other), ("admin", if admin { "1" } else { "0" })],
            )?;
            json(
                "insert into people_group_members (group_id, email, status, invited_by) values (:'g'::bigint, :'other', 'invited', :'email') \
                   on conflict do nothing; select '{\"ok\":true}'",
                &[("g", &id.to_string()), ("email", &email), ("other", &other)],
            )
        }
        InboxAct::GroupJoin { id, accept } => json(
            if *accept {
                "update people_group_members set status = 'member' where group_id = :'g'::bigint and email = :'email' and status = 'invited'; select '{\"ok\":true}'"
            } else {
                "delete from people_group_members where group_id = :'g'::bigint and email = :'email' and status = 'invited'; select '{\"ok\":true}'"
            },
            &[("g", &id.to_string()), ("email", &email)],
        ),
        InboxAct::GroupLeave { id } => json(
            // The last one out takes the group, its chat and its shares with it.
            "delete from people_group_members where group_id = :'g'::bigint and email = :'email'; \
             delete from people_groups g where g.id = :'g'::bigint and not exists (select 1 from people_group_members m where m.group_id = g.id and m.status = 'member'); \
             select '{\"ok\":true}'",
            &[("g", &id.to_string()), ("email", &email)],
        ),
        InboxAct::Share { domain, to, group } => {
            let d = domain.trim().to_ascii_lowercase();
            if !host_ok(&d) {
                return Err(format!("{d}: not a domain name"));
            }
            let other = if *group == 0 { email_of(to)? } else { String::new() };
            // Mine to share (the registry's owner or a seat, as auth_svc
            // reads it), to someone connected with me or a group I am in. A
            // person accepts it; a group's members accepted the group.
            checked(
                "select case \
                   when :'admin' <> '1' and not exists (select 1 from domain_registry where domain = :'d' and lower(owner_email) = :'email') \
                        and not exists (select 1 from user_domains where domain = :'d' and lower(email) = :'email') \
                     then json_build_object('error', :'d' || ' is not yours to share') \
                   when :'g' = '0' and :'other' = :'email' then json_build_object('error', 'that is you') \
                   when :'g' = '0' and :'admin' <> '1' and not exists (select 1 from connections where status = 'accepted' \
                          and least(from_email, to_email) = least(:'other', :'email') and greatest(from_email, to_email) = greatest(:'other', :'email')) \
                     then json_build_object('error', 'connect with them first - then you can share with them') \
                   when :'g' <> '0' and not exists (select 1 from people_group_members where group_id = :'g'::bigint and email = :'email' and status = 'member') \
                     then json_build_object('error', 'you are not in that group') \
                   else json_build_object('ok', true) end",
                &[("d", &d), ("email", &email), ("other", &other), ("g", &group.to_string()), ("admin", if admin { "1" } else { "0" })],
            )?;
            // Shared again after they turned it down: offered again.
            json(
                if *group == 0 {
                    "insert into shares (owner_email, domain, to_email) values (:'email', :'d', :'other') \
                       on conflict (domain, to_email) where to_email is not null \
                       do update set status = case when shares.status = 'declined' then 'offered' else shares.status end, owner_email = excluded.owner_email; \
                     select '{\"ok\":true}'"
                } else {
                    "insert into shares (owner_email, domain, to_group, status, decided_at) values (:'email', :'d', :'g'::bigint, 'accepted', now()) \
                       on conflict (domain, to_group) where to_group is not null do nothing; select '{\"ok\":true}'"
                },
                &[("d", &d), ("email", &email), ("other", &other), ("g", &group.to_string())],
            )
        }
        InboxAct::ShareDecide { id, accept } => json(
            "update shares set status = case when :'yes' = '1' then 'accepted' else 'declined' end, decided_at = now() \
               where id = :'id'::bigint and to_email = :'email' and status = 'offered'; select '{\"ok\":true}'",
            &[("id", &id.to_string()), ("email", &email), ("yes", if *accept { "1" } else { "0" })],
        ),
        InboxAct::ShareRevoke { id } => json(
            "delete from shares where id = :'id'::bigint and (owner_email = :'email' or :'admin' = '1'); select '{\"ok\":true}'",
            &[("id", &id.to_string()), ("email", &email), ("admin", if admin { "1" } else { "0" })],
        ),
    }
}

/// A site's address as Messages takes one: letters, digits, dots, hyphens.
fn host_ok(h: &str) -> bool {
    h.contains('.') && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

/// The email (lowercased) of the ndexr account named `username`.
fn email_of(username: &str) -> Result<String, String> {
    let v = json(
        "select json_build_object('email', coalesce((select lower(email) from ndexr_users where lower(username) = lower(:'u') and :'u' <> '' limit 1), ''))",
        &[("u", username.trim())],
    )?;
    match v["email"].as_str() {
        Some(e) if !e.is_empty() => Ok(e.to_owned()),
        _ => Err(format!("no ndexr account named {}", username.trim())),
    }
}

/// `json`, with a refusal the SQL wrote (`{"error": …}`) as Err.
fn checked(sql: &str, vars: &[(&str, &str)]) -> Result<Value, String> {
    let v = json(sql, vars)?;
    match v["error"].as_str() {
        Some(e) => Err(e.to_owned()),
        None => Ok(v),
    }
}

/// Where Chat's files are kept: on the server that answers Messages (root,
/// 0700), one file per message, named by its id.
fn files_dir() -> Result<std::path::PathBuf, String> {
    let dir = std::path::PathBuf::from(std::env::var_os("COVE_DM_FILES").unwrap_or_else(|| "/var/lib/cove/dm-files".into()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("no place for Chat's files ({}: {e})", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

/// A file's name as sent: the last path part, no hidden or odd names.
fn file_name(name: &str) -> Option<String> {
    let n = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    (!n.is_empty() && !n.starts_with('.') && n.len() <= 200 && crate::host::upload_name_ok(n)).then(|| n.to_owned())
}

/// A file sent in Chat, for its sender or the person it went to only.
pub fn dm_file(owner: &str, id: i64) -> Result<crate::api::Attachment, String> {
    let v = json(
        &format!(
            "with {ME} select json_build_object('name', coalesce(( \
               select d.body from direct_messages d where d.id = :'id'::bigint and to_jsonb(d)->>'kind' = 'file' \
                 and (lower(d.from_email) = (select email from me) or lower(d.to_email) = (select email from me))), ''))"
        ),
        &[("owner", owner), ("id", &id.to_string())],
    )?;
    let name = v["name"].as_str().unwrap_or_default().to_owned();
    if name.is_empty() {
        return Err("no such file in your chats".into());
    }
    let bytes = std::fs::read(files_dir()?.join(id.to_string())).map_err(|e| format!("{name}: {e}"))?;
    Ok(crate::api::Attachment { name, data: crate::api::b64_encode(&bytes) })
}

fn update_as(owner: &str, id: i64, status: &str, reply: &str, by: &str) -> Result<Value, String> {
    let v = crate::host::platform_r_with_file(owner, "inbox-update.R", UPDATE_R, &id.to_string(), status, reply, by)?;
    match v["error"].as_str() {
        Some(e) => Err(e.to_owned()),
        None => Ok(serde_json::json!({ "ok": true })),
    }
}

#[cfg(test)]
mod tests {
    /// Who the live tests act as: real accounts, named in the environment
    /// (COVE_LIVE_ADMIN, COVE_LIVE_PERSON, ...) rather than here.
    fn live(var: &str) -> String {
        std::env::var(var).unwrap_or_else(|_| panic!("set {var} to run the live tests"))
    }

    /// Against the live database (`cargo test inbox_live -- --ignored`, with
    /// COVE_LIVE_ADMIN an admin's username on a gmail.com address and
    /// COVE_LIVE_PERSON a person's): an admin sees everyone's requests with
    /// who sent them; a person sees only their own, and no emails.
    #[test]
    #[ignore]
    fn inbox_live() {
        let (op, person) = (live("COVE_LIVE_ADMIN"), live("COVE_LIVE_PERSON"));
        let (op, person) = (op.as_str(), person.as_str());
        let admin = super::inbox(op, true, "", "").expect("admin inbox");
        eprintln!("admin: me={} admin={} public={} open={} requests={} people={}", admin.me, admin.admin, admin.public, admin.open, admin.requests.len(), admin.people.len());
        assert!(admin.admin && admin.requests.iter().any(|r| !r.mine && !r.email.is_empty()));
        let you = super::inbox(person, true, "", "").expect("person inbox");
        eprintln!("person: me={} requests={:?}", you.me, you.requests.iter().map(|r| (r.id, r.status.clone(), r.messages.len(), r.unread)).collect::<Vec<_>>());
        assert!(!you.admin && you.requests.iter().all(|r| r.mine && r.email.is_empty()));
        assert!(super::act(person, &crate::api::InboxAct::Status { id: 1, status: "resolved".into() }).is_err(), "not theirs");
        assert!(super::act(person, &crate::api::InboxAct::Dm { to: person.into(), body: "hi".into(), kind: String::new(), file: None }).is_err(), "not to themselves");
        // Refused before anything is written: not theirs to share, not a group they are in.
        let e = super::act(person, &crate::api::InboxAct::Share { domain: "cove.ndexr.io".into(), to: op.into(), group: 0 }).unwrap_err();
        eprintln!("share refused: {e}");
        assert!(super::act(person, &crate::api::InboxAct::Dm { to: "#999999".into(), body: "hi".into(), kind: String::new(), file: None }).is_err(), "not their group");
        // Who can find me: a gmail.com address has no organization; a choice
        // goes in and `public` follows it; then it is put back as it was.
        let before = if admin.findable.is_empty() { "private".to_owned() } else { admin.findable.clone() };
        assert!(!admin.has_org, "gmail.com is not an organization");
        assert!(super::act(op, &crate::api::InboxAct::Findable { level: "org".into() }).is_err(), "no organization to be found in");
        for level in ["public", "private"] {
            super::act(op, &crate::api::InboxAct::Findable { level: level.into() }).expect("findable");
            let v = super::inbox(op, false, "", "").unwrap();
            assert_eq!((v.findable.as_str(), v.public), (level, level == "public"));
        }
        super::act(op, &crate::api::InboxAct::Findable { level: before.clone() }).expect("put back");
        eprintln!("findable put back to {before}");
    }

    /// Against the live database, between two admin accounts
    /// (`cargo test sharing_live -- --ignored`, COVE_LIVE_A / COVE_LIVE_B
    /// their usernames, COVE_LIVE_A_EMAIL / COVE_LIVE_B_EMAIL their emails,
    /// and A owning at least one domain): connect and accept, a group
    /// with an invitation taken and a message in its chat, a domain shared
    /// and accepted - and the gate's seat query letting them in - then every
    /// row taken back out.
    #[test]
    #[ignore]
    fn sharing_live() {
        use crate::api::InboxAct as A;
        let (a, b, a_email, b_email) = (live("COVE_LIVE_A"), live("COVE_LIVE_B"), live("COVE_LIVE_A_EMAIL"), live("COVE_LIVE_B_EMAIL"));
        let (a, b) = (a.as_str(), b.as_str());
        let gate = |d: &str| {
            super::json(
                "select json_build_object('in', exists (select 1 from shares s where s.domain = :'d' and s.status = 'accepted' \
                   and (s.to_email = :'b' or s.to_group in (select group_id from people_group_members where email = :'b' and status = 'member'))))",
                &[("d", d), ("b", b_email.as_str())],
            )
            .unwrap()["in"]
                .as_bool()
                .unwrap()
        };
        super::act(a, &A::Connect { to: b.into() }).expect("ask");
        assert!(super::inbox(b, false, "", "").unwrap().connections.iter().any(|c| c.username == a && c.incoming), "b sees the ask");
        super::act(b, &A::Connection { with: a.into(), accept: true }).expect("accept");
        assert!(super::inbox(a, false, "", "").unwrap().connections.iter().any(|c| c.username == b && c.status == "accepted"));

        let g = super::act(a, &A::GroupNew { name: "sharing-live test".into() }).expect("group")["id"].as_i64().unwrap();
        super::act(a, &A::GroupInvite { id: g, who: b.into() }).expect("invite");
        assert!(super::inbox(b, false, "", "").unwrap().groups.iter().any(|x| x.id == g && !x.member), "b is invited");
        super::act(b, &A::GroupJoin { id: g, accept: true }).expect("join");
        super::act(b, &A::Dm { to: format!("#{g}"), body: "hello group".into(), kind: String::new(), file: None }).expect("group message");
        let chat = super::inbox(a, false, &format!("#{g}"), "").unwrap().chat;
        assert!(chat.iter().any(|m| m.body == "hello group" && m.who == b && !m.mine), "a reads it: {chat:?}");

        let d = super::json("select json_build_object('d', (select domain from domain_registry where lower(owner_email) = lower(:'a') order by domain limit 1))", &[("a", a_email.as_str())]).unwrap()["d"]
            .as_str()
            .unwrap()
            .to_owned();
        super::act(a, &A::Share { domain: d.clone(), to: b.into(), group: 0 }).expect("share");
        assert!(!gate(&d), "offered is not in yet");
        let sh = super::inbox(b, false, "", "").unwrap().shares.into_iter().find(|s| s.domain == d && !s.mine).expect("b sees the offer");
        super::act(b, &A::ShareDecide { id: sh.id, accept: true }).expect("take it");
        assert!(gate(&d), "accepted is a seat");
        super::act(a, &A::ShareRevoke { id: sh.id }).expect("take back");
        assert!(!gate(&d), "taken back is out");
        // A group share is in for its members at once.
        super::act(a, &A::Share { domain: d.clone(), to: String::new(), group: g }).expect("share with the group");
        assert!(gate(&d), "a member of the group is in");
        // (Not-mine-to-share is not tried here: both accounts are admins, and
        // an admin may share any domain.)

        super::act(b, &A::GroupLeave { id: g }).unwrap();
        assert!(!gate(&d), "out of the group, out of the domain");
        super::act(a, &A::GroupLeave { id: g }).unwrap();
        super::act(a, &A::Connection { with: b.into(), accept: false }).unwrap();
        let left = super::json(
            "select json_build_object('n', (select count(*) from people_groups where id = :'g'::bigint) + (select count(*) from shares where domain = :'d' and owner_email = :'a') \
               + (select count(*) from connections where :'b' in (from_email, to_email) and :'a' in (from_email, to_email)))",
            &[("g", &g.to_string()), ("d", &d), ("a", a_email.as_str()), ("b", b_email.as_str())],
        )
        .unwrap();
        assert_eq!(left["n"].as_i64(), Some(0), "nothing left behind");
    }
}

/// Set who can find me (InboxAct::Findable); `public` follows for everything
/// that reads the old switch.
fn set_findable(email: &str, level: &str) -> Result<Value, String> {
    if !["private", "org", "public"].contains(&level) {
        return Err(format!("{level}: not a choice - private, org or public"));
    }
    let dom = email.rsplit('@').next().unwrap_or("");
    if level == "org" && ORG_EXCLUDED.contains(&format!("'{dom}'")) {
        return Err(format!("{dom} is shared by strangers, not an organization - choose Nobody or Everyone"));
    }
    json(
        "insert into user_presence (email, username, findable, public, last_seen) \
           select :'email', coalesce((select username from ndexr_users where lower(email) = :'email' limit 1), ''), :'level', :'level' = 'public', now() \
         on conflict (email) do update set findable = excluded.findable, public = excluded.public, last_seen = now(); select '{\"ok\":true}'",
        &[("email", email), ("level", level)],
    )
}
