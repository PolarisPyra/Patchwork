-- Stored JSON written before fields were renamed takes the new names, so the app no longer has
-- to read the old ones. A value already under its new name is kept over the old one.

-- Settings: renamed fields.
UPDATE settings SET json = json_remove(
    CASE WHEN json_type(json, '$.quiltsBackground') IS NULL
        THEN json_set(json, '$.quiltsBackground', json -> '$.artifactsBackground') ELSE json END,
    '$.artifactsBackground')
 WHERE json_valid(json) AND json_type(json, '$.artifactsBackground') IS NOT NULL;
UPDATE settings SET json = json_remove(
    CASE WHEN json_type(json, '$.hooksDir') IS NULL
        THEN json_set(json, '$.hooksDir', json -> '$.pluginsDir') ELSE json END,
    '$.pluginsDir')
 WHERE json_valid(json) AND json_type(json, '$.pluginsDir') IS NOT NULL;
UPDATE settings SET json = json_remove(
    CASE WHEN json_type(json, '$.disabledHooks') IS NULL
        THEN json_set(json, '$.disabledHooks', json -> '$.disabledPlugins') ELSE json END,
    '$.disabledPlugins')
 WHERE json_valid(json) AND json_type(json, '$.disabledPlugins') IS NOT NULL;
UPDATE settings SET json = json_remove(
    CASE WHEN json_type(json, '$.logging.chats') IS NULL
        THEN json_set(json, '$.logging.chats', json -> '$.logging.threads') ELSE json END,
    '$.logging.threads')
 WHERE json_valid(json) AND json_type(json, '$.logging.threads') IS NOT NULL;
UPDATE settings SET json = json_remove(
    CASE WHEN json_type(json, '$.logging.chatMenu') IS NULL
        THEN json_set(json, '$.logging.chatMenu', json -> '$.logging.threadMenu') ELSE json END,
    '$.logging.threadMenu')
 WHERE json_valid(json) AND json_type(json, '$.logging.threadMenu') IS NOT NULL;
UPDATE settings SET json = json_remove(
    CASE WHEN json_type(json, '$.notifications.chats') IS NULL
        THEN json_set(json, '$.notifications.chats', json -> '$.notifications.threads') ELSE json END,
    '$.notifications.threads')
 WHERE json_valid(json) AND json_type(json, '$.notifications.threads') IS NOT NULL;

-- Settings: fields that were read once and folded into others.
UPDATE settings SET json = json_remove(json, '$.agent')
 WHERE json_valid(json) AND json_type(json, '$.agent') IS NOT NULL;
UPDATE settings SET json = json_patch(json, '{"logging":{"imports":"off"}}')
 WHERE json_valid(json) AND json_type(json, '$.importLogDisabled') = 'true';
UPDATE settings SET json = json_remove(json, '$.importLogDisabled')
 WHERE json_valid(json) AND json_type(json, '$.importLogDisabled') IS NOT NULL;
UPDATE settings SET json = json_set(json, '$.vision',
        json_object('agent', 'api', 'model', json ->> '$.visionModel'))
 WHERE json_valid(json) AND json_type(json, '$.visionModel') = 'text'
   AND coalesce(json_type(json, '$.vision'), 'null') = 'null';
UPDATE settings SET json = json_remove(json, '$.visionModel')
 WHERE json_valid(json) AND json_type(json, '$.visionModel') IS NOT NULL;

-- Background pictures were named by `path`; the app keeps the file name of whatever it holds.
UPDATE settings SET json = json_remove(
    json_set(json, '$.chatBackground.file', json -> '$.chatBackground.path'), '$.chatBackground.path')
 WHERE json_valid(json) AND json_type(json, '$.chatBackground.path') IS NOT NULL
   AND json_type(json, '$.chatBackground.file') IS NULL;
UPDATE settings SET json = json_remove(
    json_set(json, '$.quiltsBackground.file', json -> '$.quiltsBackground.path'), '$.quiltsBackground.path')
 WHERE json_valid(json) AND json_type(json, '$.quiltsBackground.path') IS NOT NULL
   AND json_type(json, '$.quiltsBackground.file') IS NULL;
UPDATE agents SET background = json_remove(
    json_set(background, '$.file', background -> '$.path'), '$.path')
 WHERE json_valid(background) AND json_type(background, '$.path') IS NOT NULL
   AND json_type(background, '$.file') IS NULL;
UPDATE app_state SET value = json_remove(json_set(value, '$.file', value -> '$.path'), '$.path')
 WHERE key LIKE 'chat-background:%' AND json_valid(value)
   AND json_type(value, '$.path') IS NOT NULL AND json_type(value, '$.file') IS NULL;

-- The Orchestrator's backdrop: the butterfly was `theme`, and a scene's id was its `theme`.
UPDATE settings SET json = json_set(json, '$.orchestratorBackdrop.kind', 'butterfly')
 WHERE json_valid(json) AND json ->> '$.orchestratorBackdrop.kind' = 'theme';
UPDATE settings SET json = json_set(json, '$.systemLightBackdrop.kind', 'butterfly')
 WHERE json_valid(json) AND json ->> '$.systemLightBackdrop.kind' = 'theme';
UPDATE settings SET json = json_set(json, '$.systemDarkBackdrop.kind', 'butterfly')
 WHERE json_valid(json) AND json ->> '$.systemDarkBackdrop.kind' = 'theme';
UPDATE settings SET json = json_remove(
    json_set(json, '$.orchestratorBackdrop.id', json -> '$.orchestratorBackdrop.theme'),
    '$.orchestratorBackdrop.theme')
 WHERE json_valid(json) AND json ->> '$.orchestratorBackdrop.kind' = 'scene'
   AND json_type(json, '$.orchestratorBackdrop.theme') IS NOT NULL
   AND json_type(json, '$.orchestratorBackdrop.id') IS NULL;
UPDATE settings SET json = json_remove(
    json_set(json, '$.systemLightBackdrop.id', json -> '$.systemLightBackdrop.theme'),
    '$.systemLightBackdrop.theme')
 WHERE json_valid(json) AND json ->> '$.systemLightBackdrop.kind' = 'scene'
   AND json_type(json, '$.systemLightBackdrop.theme') IS NOT NULL
   AND json_type(json, '$.systemLightBackdrop.id') IS NULL;
UPDATE settings SET json = json_remove(
    json_set(json, '$.systemDarkBackdrop.id', json -> '$.systemDarkBackdrop.theme'),
    '$.systemDarkBackdrop.theme')
 WHERE json_valid(json) AND json ->> '$.systemDarkBackdrop.kind' = 'scene'
   AND json_type(json, '$.systemDarkBackdrop.theme') IS NOT NULL
   AND json_type(json, '$.systemDarkBackdrop.id') IS NULL;

-- Chats: a subagent's parent, and goals the agents' own usage and budget stops paused.
UPDATE chats SET parent = json_remove(json_set(parent, '$.chatId', parent -> '$.threadId'), '$.threadId')
 WHERE json_valid(parent) AND json_type(parent, '$.threadId') IS NOT NULL
   AND json_type(parent, '$.chatId') IS NULL;
UPDATE chats SET goal = json_set(goal, '$.status', 'paused')
 WHERE json_valid(goal) AND goal ->> '$.status' IN ('usageLimited', 'budgetLimited');

-- Messages: the reviewer's own chat.
UPDATE messages SET review = json_remove(json_set(review, '$.chatId', review -> '$.threadId'), '$.threadId')
 WHERE json_valid(review) AND json_type(review, '$.threadId') IS NOT NULL
   AND json_type(review, '$.chatId') IS NULL;

-- App state: what was recorded about a commit, and the chat app conversations bound to chats.
UPDATE app_state SET value = json_remove(json_set(value, '$.chat', value -> '$.thread'), '$.thread')
 WHERE key LIKE 'commit-chat:%' AND json_valid(value)
   AND json_type(value, '$.thread') IS NOT NULL AND json_type(value, '$.chat') IS NULL;
UPDATE app_state SET value = (
    SELECT json_group_array(
        CASE WHEN e.type <> 'object' THEN app_state.value -> e.fullkey
            WHEN json_type(e.value, '$.threadId') IS NULL OR json_type(e.value, '$.chatId') IS NOT NULL
            THEN app_state.value -> e.fullkey
            ELSE json_remove(json_set(e.value, '$.chatId', e.value -> '$.threadId'), '$.threadId') END
        ORDER BY e.key)
      FROM json_each(app_state.value) AS e)
 WHERE key IN ('gateway.bindings', 'gateway.watches') AND json_valid(value)
   AND json_type(value) = 'array';
