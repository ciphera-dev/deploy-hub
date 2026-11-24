--
-- PostgreSQL database dump
--

\restrict wjbUEAso2pBrHQ5ayMrs9OtLP6ZwzECIq3ctToSUtZVoIc2Fm2xxbtq2WMW8LUS

-- Dumped from database version 17.12 (Debian 17.12-1.pgdg13+1)
-- Dumped by pg_dump version 17.12 (Debian 17.12-1.pgdg13+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: _sqlx_migrations; Type: TABLE; Schema: public; Owner: ciphera
--

CREATE TABLE public._sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    success boolean NOT NULL
);


ALTER TABLE public._sqlx_migrations OWNER TO ciphera;

--
-- Name: projects; Type: TABLE; Schema: public; Owner: ciphera
--

CREATE TABLE public.projects (
    id integer NOT NULL,
    name text NOT NULL
);


ALTER TABLE public.projects OWNER TO ciphera;

--
-- Data for Name: _sqlx_migrations; Type: TABLE DATA; Schema: public; Owner: ciphera
--

COPY public._sqlx_migrations (version, description, success) FROM stdin;
1	initial	t
2	projects	t
\.


--
-- Data for Name: projects; Type: TABLE DATA; Schema: public; Owner: ciphera
--

COPY public.projects (id, name) FROM stdin;
1	demo
\.


--
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: ciphera
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


--
-- Name: projects projects_pkey; Type: CONSTRAINT; Schema: public; Owner: ciphera
--

ALTER TABLE ONLY public.projects
    ADD CONSTRAINT projects_pkey PRIMARY KEY (id);


--
-- PostgreSQL database dump complete
--

\unrestrict wjbUEAso2pBrHQ5ayMrs9OtLP6ZwzECIq3ctToSUtZVoIc2Fm2xxbtq2WMW8LUS

