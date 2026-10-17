-- The SRP wallets, one per SRP corp: the SeAT login of a character in that corp, used only by the
-- SRP check. Kept apart from refresh_token, which is keyed by main and replaced on every ordinary
-- login, so an admin logging in normally can't drop the wallet scopes.
CREATE TABLE
    "public"."srp_connection" (
        "corporation_id" int8 NOT NULL,
        "character_id" int8 NOT NULL,
        "corporation_name" varchar NOT NULL,
        "refresh_token" text NOT NULL,
        "connected_by" int8 NOT NULL,
        "connected_at" int8 NOT NULL,
        PRIMARY KEY ("corporation_id"),
        CONSTRAINT "fk_srp_connection_character" FOREIGN KEY ("character_id") REFERENCES "public"."character" ("id"),
        CONSTRAINT "fk_srp_connection_connected_by" FOREIGN KEY ("connected_by") REFERENCES "public"."character" ("id")
    );

-- Which SRP corp's payments cover each waitlist's pilots, set on the SRP wallet page. No foreign key
-- to srp_connection: disconnecting a wallet keeps its waitlists' setting until it's reconnected.
CREATE TABLE
    "public"."srp_waitlist" (
        "waitlist_id" int8 NOT NULL,
        "corporation_id" int8 NOT NULL,
        "set_by" int8 NOT NULL,
        "set_at" int8 NOT NULL,
        PRIMARY KEY ("waitlist_id"),
        CONSTRAINT "fk_srp_waitlist_waitlist" FOREIGN KEY ("waitlist_id") REFERENCES "public"."waitlist" ("id") ON DELETE CASCADE,
        CONSTRAINT "fk_srp_waitlist_set_by" FOREIGN KEY ("set_by") REFERENCES "public"."character" ("id")
    );
