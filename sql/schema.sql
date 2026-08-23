--
-- PostgreSQL database dump
--

\restrict L2lLxPaJu3e6JDmYe6LdCSIfWBah7LotrPQbBo3kXf5cak4q14IhWeCWnM7zPzH

-- Dumped from database version 16.13 (Debian 16.13-1.pgdg13+1)
-- Dumped by pg_dump version 16.13 (Debian 16.13-1.pgdg13+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: vector; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;


--
-- Name: EXTENSION vector; Type: COMMENT; Schema: -; Owner: -
--

COMMENT ON EXTENSION vector IS 'vector data type and ivfflat and hnsw access methods';


--
-- Name: memory_fts_update(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.memory_fts_update() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
            BEGIN
                NEW.fts_doc :=
                    setweight(to_tsvector('simple', COALESCE(NEW.title,'')), 'A') ||
                    setweight(to_tsvector('simple', COALESCE(NEW.summary,'')), 'B') ||
                    setweight(to_tsvector('simple', COALESCE(NEW.path,'')), 'C');
                RETURN NEW;
            END
            $$;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: chat_message; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_message (
    id bigint NOT NULL,
    session_id text NOT NULL,
    role text NOT NULL,
    content text NOT NULL,
    created_at bigint NOT NULL
);


--
-- Name: chat_message_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.chat_message_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: chat_message_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.chat_message_id_seq OWNED BY public.chat_message.id;


--
-- Name: chat_session; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_session (
    id text NOT NULL,
    kb_id text NOT NULL,
    name text NOT NULL,
    created_at bigint NOT NULL,
    updated_at bigint NOT NULL
);


--
-- Name: domain_data; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.domain_data (
    key text NOT NULL,
    value text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP
);


--
-- Name: domain_kb; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.domain_kb (
    id text NOT NULL,
    name text NOT NULL,
    description text DEFAULT ''::text NOT NULL,
    created_at bigint NOT NULL,
    updated_at bigint NOT NULL,
    expert_id text DEFAULT ''::text NOT NULL,
    tags jsonb DEFAULT '[]'::jsonb NOT NULL
);


--
-- Name: engine_run_log; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.engine_run_log (
    id bigint NOT NULL,
    kb_id text NOT NULL,
    session_id text,
    query_hash text DEFAULT ''::text NOT NULL,
    query text NOT NULL,
    task_success boolean DEFAULT true NOT NULL,
    output text DEFAULT ''::text NOT NULL,
    created_at bigint NOT NULL
);


--
-- Name: engine_run_log_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.engine_run_log_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: engine_run_log_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.engine_run_log_id_seq OWNED BY public.engine_run_log.id;


--
-- Name: graph_edges; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.graph_edges (
    from_entity text NOT NULL,
    to_entity text NOT NULL,
    edge_kind text NOT NULL,
    weight real DEFAULT 0.3 NOT NULL
);


--
-- Name: graph_entity_chunks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.graph_entity_chunks (
    entity_id text NOT NULL,
    chunk_id text NOT NULL
);


--
-- Name: graph_entity_feedback; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.graph_entity_feedback (
    entity_id text NOT NULL,
    feedback_score real DEFAULT 0.0 NOT NULL
);


--
-- Name: kb_chunk; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.kb_chunk (
    id text NOT NULL,
    kb_id text NOT NULL,
    title text DEFAULT ''::text NOT NULL,
    content text NOT NULL,
    content_hash text DEFAULT ''::text NOT NULL,
    status text DEFAULT 'active'::text NOT NULL,
    sort_order integer DEFAULT 0 NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at bigint NOT NULL,
    updated_at bigint NOT NULL
);


--
-- Name: kg_entity; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.kg_entity (
    id text NOT NULL,
    kb_id text NOT NULL,
    name text NOT NULL,
    entity_type text DEFAULT 'default'::text NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at bigint NOT NULL
);


--
-- Name: kg_relation; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.kg_relation (
    id bigint NOT NULL,
    kb_id text NOT NULL,
    from_entity text NOT NULL,
    to_entity text NOT NULL,
    relation_type text NOT NULL,
    weight real DEFAULT 0.3 NOT NULL,
    created_at bigint NOT NULL
);


--
-- Name: kg_relation_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.kg_relation_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: kg_relation_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.kg_relation_id_seq OWNED BY public.kg_relation.id;


--
-- Name: knowledge_tree_node; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.knowledge_tree_node (
    id text NOT NULL,
    kb_id text NOT NULL,
    parent_id text,
    node_name character varying(255) NOT NULL,
    node_desc text,
    sort_index integer DEFAULT 0 NOT NULL,
    created_at bigint NOT NULL,
    updated_at bigint NOT NULL
);


--
-- Name: memories; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memories (
    id integer NOT NULL,
    user_id character varying(255) NOT NULL,
    role character varying(20) NOT NULL,
    content text NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    archived boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    session_id character varying(255),
    layer character varying(20) DEFAULT 'short_term'::character varying NOT NULL,
    embedding public.vector(384)
);


--
-- Name: memories_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.memories_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: memories_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.memories_id_seq OWNED BY public.memories.id;


--
-- Name: memory_collections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memory_collections (
    collection_id text NOT NULL,
    name text NOT NULL,
    domain text NOT NULL,
    description text,
    created_at bigint NOT NULL
);


--
-- Name: memory_edges; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memory_edges (
    edge_id text NOT NULL,
    from_node_id text NOT NULL,
    from_collection_id text NOT NULL,
    to_node_id text NOT NULL,
    to_collection_id text NOT NULL,
    edge_type text NOT NULL,
    weight real DEFAULT 1.0 NOT NULL,
    created_at bigint NOT NULL
);


--
-- Name: memory_nodes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memory_nodes (
    node_id text NOT NULL,
    collection_id text NOT NULL,
    domain text NOT NULL,
    node_type text NOT NULL,
    parent_id text,
    path text NOT NULL,
    depth integer NOT NULL,
    sort_order integer DEFAULT 0 NOT NULL,
    title text NOT NULL,
    summary text NOT NULL,
    content text NOT NULL,
    content_hash text NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb,
    version_tag text DEFAULT 'current'::text NOT NULL,
    snapshot_id text,
    base_activation real DEFAULT 0.5 NOT NULL,
    importance smallint DEFAULT 1 NOT NULL,
    access_count integer DEFAULT 0 NOT NULL,
    feedback_score real DEFAULT 0.0 NOT NULL,
    last_accessed_at bigint NOT NULL,
    created_at bigint NOT NULL,
    updated_at bigint NOT NULL,
    fts_doc tsvector
);


--
-- Name: memory_snapshots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memory_snapshots (
    snapshot_id text NOT NULL,
    collection_id text NOT NULL,
    name text NOT NULL,
    version_tag text NOT NULL,
    description text,
    created_at bigint NOT NULL
);


--
-- Name: persona_history; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.persona_history (
    id integer NOT NULL,
    user_id character varying(255) NOT NULL,
    version integer NOT NULL,
    profile_snapshot jsonb NOT NULL,
    reason text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: persona_history_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.persona_history_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: persona_history_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.persona_history_id_seq OWNED BY public.persona_history.id;


--
-- Name: persona_profiles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.persona_profiles (
    id integer NOT NULL,
    user_id character varying(255) NOT NULL,
    version integer DEFAULT 1 NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    tone character varying(50) NOT NULL,
    emotional_tendency character varying(50) NOT NULL,
    openness real DEFAULT 0.6 NOT NULL,
    conscientiousness real DEFAULT 0.5 NOT NULL,
    extraversion real DEFAULT 0.5 NOT NULL,
    agreeableness real DEFAULT 0.7 NOT NULL,
    neuroticism real DEFAULT 0.4 NOT NULL,
    traits jsonb DEFAULT '[]'::jsonb NOT NULL,
    skill_proficiency jsonb DEFAULT '{}'::jsonb NOT NULL,
    expertise_areas jsonb DEFAULT '{}'::jsonb NOT NULL,
    skill_affinity jsonb DEFAULT '{}'::jsonb NOT NULL,
    total_interactions integer DEFAULT 0 NOT NULL,
    likes integer DEFAULT 0 NOT NULL,
    dislikes integer DEFAULT 0 NOT NULL,
    avg_response_time_ms bigint DEFAULT 0 NOT NULL,
    skill_usage jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: persona_profiles_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.persona_profiles_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: persona_profiles_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.persona_profiles_id_seq OWNED BY public.persona_profiles.id;


--
-- Name: sutra_execution_logs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sutra_execution_logs (
    id integer NOT NULL,
    query_hash character varying(64) NOT NULL,
    query text NOT NULL,
    recalled_chunks jsonb DEFAULT '[]'::jsonb NOT NULL,
    used_chunk_ids text[] DEFAULT '{}'::text[] NOT NULL,
    task_success boolean DEFAULT true NOT NULL,
    "timestamp" bigint NOT NULL,
    graph character varying(64) DEFAULT 'default'::character varying NOT NULL,
    domain character varying(64) DEFAULT 'general'::character varying NOT NULL,
    session_id character varying(64) DEFAULT NULL::character varying
);


--
-- Name: sutra_execution_logs_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.sutra_execution_logs_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sutra_execution_logs_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.sutra_execution_logs_id_seq OWNED BY public.sutra_execution_logs.id;


--
-- Name: sutra_feedback_metrics; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sutra_feedback_metrics (
    id integer NOT NULL,
    graph character varying(64) NOT NULL,
    domain character varying(64) NOT NULL,
    total_queries bigint DEFAULT 0 NOT NULL,
    hit_rate double precision DEFAULT 0.0 NOT NULL,
    source_contribution jsonb DEFAULT '{}'::jsonb NOT NULL,
    avg_used_chunks double precision DEFAULT 0.0 NOT NULL,
    success_rate double precision DEFAULT 0.0 NOT NULL,
    analyzed_at bigint NOT NULL
);


--
-- Name: sutra_feedback_metrics_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.sutra_feedback_metrics_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sutra_feedback_metrics_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.sutra_feedback_metrics_id_seq OWNED BY public.sutra_feedback_metrics.id;


--
-- Name: temp_knowledge_buffer; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.temp_knowledge_buffer (
    id bigint NOT NULL,
    kb_id text NOT NULL,
    content text NOT NULL,
    source text DEFAULT 'manual'::text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at bigint NOT NULL
);


--
-- Name: temp_knowledge_buffer_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.temp_knowledge_buffer_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: temp_knowledge_buffer_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.temp_knowledge_buffer_id_seq OWNED BY public.temp_knowledge_buffer.id;


--
-- Name: tree_chunk_link; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.tree_chunk_link (
    id text NOT NULL,
    kb_id text NOT NULL,
    tree_node_id text NOT NULL,
    chunk_id text NOT NULL,
    created_at bigint NOT NULL
);


--
-- Name: user_feedbacks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_feedbacks (
    id integer NOT NULL,
    user_id character varying(255) NOT NULL,
    feedback_type character varying(20) NOT NULL,
    content text,
    skill_name character varying(255) NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: user_feedbacks_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.user_feedbacks_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: user_feedbacks_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.user_feedbacks_id_seq OWNED BY public.user_feedbacks.id;


--
-- Name: chat_message id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_message ALTER COLUMN id SET DEFAULT nextval('public.chat_message_id_seq'::regclass);


--
-- Name: engine_run_log id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_run_log ALTER COLUMN id SET DEFAULT nextval('public.engine_run_log_id_seq'::regclass);


--
-- Name: kg_relation id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kg_relation ALTER COLUMN id SET DEFAULT nextval('public.kg_relation_id_seq'::regclass);


--
-- Name: memories id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memories ALTER COLUMN id SET DEFAULT nextval('public.memories_id_seq'::regclass);


--
-- Name: persona_history id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.persona_history ALTER COLUMN id SET DEFAULT nextval('public.persona_history_id_seq'::regclass);


--
-- Name: persona_profiles id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.persona_profiles ALTER COLUMN id SET DEFAULT nextval('public.persona_profiles_id_seq'::regclass);


--
-- Name: sutra_execution_logs id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sutra_execution_logs ALTER COLUMN id SET DEFAULT nextval('public.sutra_execution_logs_id_seq'::regclass);


--
-- Name: sutra_feedback_metrics id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sutra_feedback_metrics ALTER COLUMN id SET DEFAULT nextval('public.sutra_feedback_metrics_id_seq'::regclass);


--
-- Name: temp_knowledge_buffer id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.temp_knowledge_buffer ALTER COLUMN id SET DEFAULT nextval('public.temp_knowledge_buffer_id_seq'::regclass);


--
-- Name: user_feedbacks id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_feedbacks ALTER COLUMN id SET DEFAULT nextval('public.user_feedbacks_id_seq'::regclass);


--
-- Name: chat_message chat_message_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_message
    ADD CONSTRAINT chat_message_pkey PRIMARY KEY (id);


--
-- Name: chat_session chat_session_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_session
    ADD CONSTRAINT chat_session_pkey PRIMARY KEY (id);


--
-- Name: domain_data domain_data_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.domain_data
    ADD CONSTRAINT domain_data_pkey PRIMARY KEY (key);


--
-- Name: domain_kb domain_kb_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.domain_kb
    ADD CONSTRAINT domain_kb_pkey PRIMARY KEY (id);


--
-- Name: engine_run_log engine_run_log_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_run_log
    ADD CONSTRAINT engine_run_log_pkey PRIMARY KEY (id);


--
-- Name: graph_edges graph_edges_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.graph_edges
    ADD CONSTRAINT graph_edges_pkey PRIMARY KEY (from_entity, to_entity, edge_kind);


--
-- Name: graph_entity_chunks graph_entity_chunks_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.graph_entity_chunks
    ADD CONSTRAINT graph_entity_chunks_pkey PRIMARY KEY (entity_id, chunk_id);


--
-- Name: graph_entity_feedback graph_entity_feedback_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.graph_entity_feedback
    ADD CONSTRAINT graph_entity_feedback_pkey PRIMARY KEY (entity_id);


--
-- Name: kb_chunk kb_chunk_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kb_chunk
    ADD CONSTRAINT kb_chunk_pkey PRIMARY KEY (id);


--
-- Name: kg_entity kg_entity_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kg_entity
    ADD CONSTRAINT kg_entity_pkey PRIMARY KEY (id);


--
-- Name: kg_relation kg_relation_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kg_relation
    ADD CONSTRAINT kg_relation_pkey PRIMARY KEY (id);


--
-- Name: knowledge_tree_node knowledge_tree_node_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_tree_node
    ADD CONSTRAINT knowledge_tree_node_pkey PRIMARY KEY (id);


--
-- Name: memories memories_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memories
    ADD CONSTRAINT memories_pkey PRIMARY KEY (id);


--
-- Name: memory_collections memory_collections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_collections
    ADD CONSTRAINT memory_collections_pkey PRIMARY KEY (collection_id);


--
-- Name: memory_edges memory_edges_from_node_id_to_node_id_edge_type_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_edges
    ADD CONSTRAINT memory_edges_from_node_id_to_node_id_edge_type_key UNIQUE (from_node_id, to_node_id, edge_type);


--
-- Name: memory_edges memory_edges_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_edges
    ADD CONSTRAINT memory_edges_pkey PRIMARY KEY (edge_id);


--
-- Name: memory_nodes memory_nodes_collection_id_version_tag_path_node_type_title_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_nodes
    ADD CONSTRAINT memory_nodes_collection_id_version_tag_path_node_type_title_key UNIQUE (collection_id, version_tag, path, node_type, title);


--
-- Name: memory_nodes memory_nodes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_nodes
    ADD CONSTRAINT memory_nodes_pkey PRIMARY KEY (node_id);


--
-- Name: memory_snapshots memory_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_snapshots
    ADD CONSTRAINT memory_snapshots_pkey PRIMARY KEY (snapshot_id);


--
-- Name: persona_history persona_history_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.persona_history
    ADD CONSTRAINT persona_history_pkey PRIMARY KEY (id);


--
-- Name: persona_profiles persona_profiles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.persona_profiles
    ADD CONSTRAINT persona_profiles_pkey PRIMARY KEY (id);


--
-- Name: persona_profiles persona_profiles_user_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.persona_profiles
    ADD CONSTRAINT persona_profiles_user_id_key UNIQUE (user_id);


--
-- Name: sutra_execution_logs sutra_execution_logs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sutra_execution_logs
    ADD CONSTRAINT sutra_execution_logs_pkey PRIMARY KEY (id);


--
-- Name: sutra_feedback_metrics sutra_feedback_metrics_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sutra_feedback_metrics
    ADD CONSTRAINT sutra_feedback_metrics_pkey PRIMARY KEY (id);


--
-- Name: temp_knowledge_buffer temp_knowledge_buffer_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.temp_knowledge_buffer
    ADD CONSTRAINT temp_knowledge_buffer_pkey PRIMARY KEY (id);


--
-- Name: tree_chunk_link tree_chunk_link_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tree_chunk_link
    ADD CONSTRAINT tree_chunk_link_pkey PRIMARY KEY (id);


--
-- Name: tree_chunk_link tree_chunk_link_tree_node_id_chunk_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tree_chunk_link
    ADD CONSTRAINT tree_chunk_link_tree_node_id_chunk_id_key UNIQUE (tree_node_id, chunk_id);


--
-- Name: user_feedbacks user_feedbacks_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_feedbacks
    ADD CONSTRAINT user_feedbacks_pkey PRIMARY KEY (id);


--
-- Name: idx_chat_message_session; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_chat_message_session ON public.chat_message USING btree (session_id);


--
-- Name: idx_chat_session_kb; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_chat_session_kb ON public.chat_session USING btree (kb_id);


--
-- Name: idx_domain_data_key; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_domain_data_key ON public.domain_data USING btree (key);


--
-- Name: idx_edges_from; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_edges_from ON public.memory_edges USING btree (from_node_id);


--
-- Name: idx_edges_to; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_edges_to ON public.memory_edges USING btree (to_node_id);


--
-- Name: idx_engine_run_log_kb; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_run_log_kb ON public.engine_run_log USING btree (kb_id);


--
-- Name: idx_engine_run_log_session; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_run_log_session ON public.engine_run_log USING btree (session_id);


--
-- Name: idx_engine_run_log_ts; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_run_log_ts ON public.engine_run_log USING btree (created_at);


--
-- Name: idx_feedbacks_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_feedbacks_user_id ON public.user_feedbacks USING btree (user_id);


--
-- Name: idx_graph_edges_to; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_graph_edges_to ON public.graph_edges USING btree (to_entity);


--
-- Name: idx_graph_entity_chunks_chunk; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_graph_entity_chunks_chunk ON public.graph_entity_chunks USING btree (chunk_id);


--
-- Name: idx_history_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_history_user_id ON public.persona_history USING btree (user_id);


--
-- Name: idx_kb_chunk_hash; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_kb_chunk_hash ON public.kb_chunk USING btree (content_hash);


--
-- Name: idx_kb_chunk_kb; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_kb_chunk_kb ON public.kb_chunk USING btree (kb_id);


--
-- Name: idx_kb_chunk_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_kb_chunk_status ON public.kb_chunk USING btree (status);


--
-- Name: idx_kg_entity_kb; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_kg_entity_kb ON public.kg_entity USING btree (kb_id);


--
-- Name: idx_kg_entity_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_kg_entity_name ON public.kg_entity USING btree (name);


--
-- Name: idx_kg_relation_from; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_kg_relation_from ON public.kg_relation USING btree (from_entity);


--
-- Name: idx_kg_relation_kb; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_kg_relation_kb ON public.kg_relation USING btree (kb_id);


--
-- Name: idx_kg_relation_to; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_kg_relation_to ON public.kg_relation USING btree (to_entity);


--
-- Name: idx_ktn_kb; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ktn_kb ON public.knowledge_tree_node USING btree (kb_id);


--
-- Name: idx_ktn_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ktn_name ON public.knowledge_tree_node USING btree (node_name);


--
-- Name: idx_ktn_parent; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ktn_parent ON public.knowledge_tree_node USING btree (parent_id);


--
-- Name: idx_memories_archived; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_memories_archived ON public.memories USING btree (archived);


--
-- Name: idx_memories_layer; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_memories_layer ON public.memories USING btree (layer);


--
-- Name: idx_memories_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_memories_user_id ON public.memories USING btree (user_id);


--
-- Name: idx_nodes_activation; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nodes_activation ON public.memory_nodes USING btree (base_activation);


--
-- Name: idx_nodes_collection; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nodes_collection ON public.memory_nodes USING btree (collection_id);


--
-- Name: idx_nodes_fts; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nodes_fts ON public.memory_nodes USING gin (fts_doc);


--
-- Name: idx_nodes_hash; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nodes_hash ON public.memory_nodes USING btree (content_hash);


--
-- Name: idx_nodes_parent; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nodes_parent ON public.memory_nodes USING btree (parent_id);


--
-- Name: idx_nodes_path; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nodes_path ON public.memory_nodes USING btree (path);


--
-- Name: idx_sutra_exec_logs_domain; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sutra_exec_logs_domain ON public.sutra_execution_logs USING btree (domain);


--
-- Name: idx_sutra_exec_logs_ts; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sutra_exec_logs_ts ON public.sutra_execution_logs USING btree ("timestamp");


--
-- Name: idx_sutra_fb_metrics_graph; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sutra_fb_metrics_graph ON public.sutra_feedback_metrics USING btree (graph, domain);


--
-- Name: idx_tcl_chunk; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tcl_chunk ON public.tree_chunk_link USING btree (chunk_id);


--
-- Name: idx_tcl_kb; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tcl_kb ON public.tree_chunk_link USING btree (kb_id);


--
-- Name: idx_tcl_node; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_tcl_node ON public.tree_chunk_link USING btree (tree_node_id);


--
-- Name: idx_temp_kb_kb; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_temp_kb_kb ON public.temp_knowledge_buffer USING btree (kb_id);


--
-- Name: idx_temp_kb_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_temp_kb_status ON public.temp_knowledge_buffer USING btree (status);


--
-- Name: memory_nodes trg_memory_fts; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_memory_fts BEFORE INSERT OR UPDATE ON public.memory_nodes FOR EACH ROW EXECUTE FUNCTION public.memory_fts_update();


--
-- Name: chat_message chat_message_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_message
    ADD CONSTRAINT chat_message_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.chat_session(id) ON DELETE CASCADE;


--
-- Name: chat_session chat_session_kb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_session
    ADD CONSTRAINT chat_session_kb_id_fkey FOREIGN KEY (kb_id) REFERENCES public.domain_kb(id) ON DELETE CASCADE;


--
-- Name: engine_run_log engine_run_log_kb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_run_log
    ADD CONSTRAINT engine_run_log_kb_id_fkey FOREIGN KEY (kb_id) REFERENCES public.domain_kb(id) ON DELETE CASCADE;


--
-- Name: engine_run_log engine_run_log_session_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_run_log
    ADD CONSTRAINT engine_run_log_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.chat_session(id) ON DELETE SET NULL;


--
-- Name: kb_chunk kb_chunk_kb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kb_chunk
    ADD CONSTRAINT kb_chunk_kb_id_fkey FOREIGN KEY (kb_id) REFERENCES public.domain_kb(id) ON DELETE CASCADE;


--
-- Name: kg_entity kg_entity_kb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kg_entity
    ADD CONSTRAINT kg_entity_kb_id_fkey FOREIGN KEY (kb_id) REFERENCES public.domain_kb(id) ON DELETE CASCADE;


--
-- Name: kg_relation kg_relation_from_entity_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kg_relation
    ADD CONSTRAINT kg_relation_from_entity_fkey FOREIGN KEY (from_entity) REFERENCES public.kg_entity(id) ON DELETE CASCADE;


--
-- Name: kg_relation kg_relation_kb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kg_relation
    ADD CONSTRAINT kg_relation_kb_id_fkey FOREIGN KEY (kb_id) REFERENCES public.domain_kb(id) ON DELETE CASCADE;


--
-- Name: kg_relation kg_relation_to_entity_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.kg_relation
    ADD CONSTRAINT kg_relation_to_entity_fkey FOREIGN KEY (to_entity) REFERENCES public.kg_entity(id) ON DELETE CASCADE;


--
-- Name: knowledge_tree_node knowledge_tree_node_kb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_tree_node
    ADD CONSTRAINT knowledge_tree_node_kb_id_fkey FOREIGN KEY (kb_id) REFERENCES public.domain_kb(id) ON DELETE CASCADE;


--
-- Name: knowledge_tree_node knowledge_tree_node_parent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.knowledge_tree_node
    ADD CONSTRAINT knowledge_tree_node_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES public.knowledge_tree_node(id) ON DELETE CASCADE;


--
-- Name: memory_nodes memory_nodes_collection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_nodes
    ADD CONSTRAINT memory_nodes_collection_id_fkey FOREIGN KEY (collection_id) REFERENCES public.memory_collections(collection_id);


--
-- Name: memory_snapshots memory_snapshots_collection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_snapshots
    ADD CONSTRAINT memory_snapshots_collection_id_fkey FOREIGN KEY (collection_id) REFERENCES public.memory_collections(collection_id);


--
-- Name: temp_knowledge_buffer temp_knowledge_buffer_kb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.temp_knowledge_buffer
    ADD CONSTRAINT temp_knowledge_buffer_kb_id_fkey FOREIGN KEY (kb_id) REFERENCES public.domain_kb(id) ON DELETE CASCADE;


--
-- Name: tree_chunk_link tree_chunk_link_chunk_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tree_chunk_link
    ADD CONSTRAINT tree_chunk_link_chunk_id_fkey FOREIGN KEY (chunk_id) REFERENCES public.kb_chunk(id) ON DELETE CASCADE;


--
-- Name: tree_chunk_link tree_chunk_link_kb_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tree_chunk_link
    ADD CONSTRAINT tree_chunk_link_kb_id_fkey FOREIGN KEY (kb_id) REFERENCES public.domain_kb(id) ON DELETE CASCADE;


--
-- Name: tree_chunk_link tree_chunk_link_tree_node_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tree_chunk_link
    ADD CONSTRAINT tree_chunk_link_tree_node_id_fkey FOREIGN KEY (tree_node_id) REFERENCES public.knowledge_tree_node(id) ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--

\unrestrict L2lLxPaJu3e6JDmYe6LdCSIfWBah7LotrPQbBo3kXf5cak4q14IhWeCWnM7zPzH

