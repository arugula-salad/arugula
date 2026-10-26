-- 0055: what comes back from GitHub to an agent session (M260, thread =
-- session, Arugula #129): a CI round or a review on its branch, gathered,
-- then sent to the session as one prompt. Times are milliseconds since the
-- epoch. `db::session_feedback` has the rules, and `sessions::feedback`
-- what sends them.
--
-- A batch (`sfb_` and 32 hex characters) of session `session_id`: `kind`
-- `ci`, the failed checks of one head commit (`batch` is its sha), or
-- `review`, one review's text and inline comments (`batch` is GitHub's id
-- for the review). `number` is the PR's, when GitHub said. `login`,
-- `github_id` and `association` are a review's author, as GitHub said
-- (`author_association`), and `review_state` its state. It's `gathering`
-- until `due_at`, then `sent` (as prompt `prompt_id`, a CI round
-- numbered `round`), `refused` (the session had its CI rounds) or
-- `dropped` (the session had ended, or a review said nothing). Each
-- batch of a session is one per kind and key, so a redelivery adds
-- nothing.
CREATE TABLE session_feedback (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    session_id TEXT COLLATE pg_c_utf8 NOT NULL,
    kind TEXT COLLATE pg_c_utf8 NOT NULL CHECK (kind IN ('ci', 'review')),
    batch TEXT COLLATE pg_c_utf8 NOT NULL,
    number BIGINT,
    login TEXT COLLATE pg_c_utf8,
    github_id BIGINT,
    association TEXT COLLATE pg_c_utf8,
    review_state TEXT COLLATE pg_c_utf8,
    state TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT 'gathering' CHECK (state IN ('gathering', 'sent', 'refused', 'dropped')),
    due_at BIGINT NOT NULL,
    round BIGINT,
    prompt_id TEXT COLLATE pg_c_utf8,
    created_at BIGINT NOT NULL,
    done_at BIGINT,
    UNIQUE (session_id, kind, batch)
);
CREATE INDEX session_feedback_due ON session_feedback (session_id, due_at) WHERE state = 'gathering';

-- What a batch gathered, each once (`item`: `check_run:<id>`,
-- `status:<id>`, `review:<id>` or `comment:<id>`, GitHub's ids): a failed
-- check's `name`, `conclusion`, `url` and the end of its output (`text`),
-- or a review's text (`name` empty) or an inline comment's (`name` its
-- file, `line` its line). GitHub's text, untrusted: it reaches the agent
-- only fenced as quoted data.
CREATE TABLE session_feedback_items (
    feedback_id TEXT COLLATE pg_c_utf8 NOT NULL,
    item TEXT COLLATE pg_c_utf8 NOT NULL,
    name TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    line BIGINT,
    conclusion TEXT COLLATE pg_c_utf8,
    url TEXT COLLATE pg_c_utf8,
    text TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    at BIGINT NOT NULL,
    PRIMARY KEY (feedback_id, item)
);
