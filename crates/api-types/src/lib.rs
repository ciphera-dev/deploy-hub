//! Contrat d'API : types de requete/reponse et document OpenAPI.
//! `openapi.json` et `web/src/lib/api/schema.d.ts` en sont generes (`just gen`).

pub mod auth;
pub mod deployments;
pub mod domains;
pub mod github;
pub mod health;
pub mod invitations;
pub mod me;
pub mod mfa;
pub mod notifications;
pub mod overview;
pub mod panel_domain;
pub mod password;
pub mod project_git;
pub mod project_overview;
pub mod projects;
pub mod settings;
pub mod setup;
pub mod system;
pub mod team;
pub mod variables;
pub mod webauthn;

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "Ciphera API", license(name = "AGPL-3.0-only")),
    paths(
        health::health,
        setup::status,
        setup::checks,
        setup::complete,
        auth::login,
        auth::logout,
        auth::me,
        mfa::totp_start,
        mfa::totp_confirm,
        mfa::totp_disable,
        mfa::mfa_verify,
        mfa::mfa_methods,
        mfa::recovery_code,
        webauthn::webauthn_status,
        webauthn::webauthn_register_options,
        webauthn::webauthn_register_verify,
        webauthn::webauthn_list,
        webauthn::webauthn_delete,
        webauthn::webauthn_assert_options,
        webauthn::webauthn_assert_verify,
        invitations::create_invitation,
        invitations::invitation_info,
        invitations::accept_invitation,
        invitations::signup_status,
        invitations::register,
        team::list_members,
        team::update_member,
        team::remove_member,
        team::list_invitations,
        team::cancel_invitation,
        password::password_reset_status,
        password::forgot_password,
        password::reset_token_info,
        password::reset_password,
        password::member_reset_link,
        notifications::get_notifications,
        notifications::put_notifications,
        me::update_me,
        me::get_avatar,
        me::put_avatar,
        me::delete_avatar,
        me::change_password,
        me::list_sessions,
        me::revoke_session,
        me::get_notification_prefs,
        me::put_notification_prefs,
        projects::list_projects,
        projects::create_project,
        projects::get_project,
        projects::update_project,
        projects::delete_project,
        github::github_status,
        github::github_manifest,
        github::github_callback,
        github::github_disconnect,
        github::github_repos,
        deployments::create_deployment,
        deployments::list_deployments,
        deployments::get_deployment,
        deployments::cancel_deployment,
        deployments::rollback_deployment,
        project_git::get_git_settings,
        project_git::update_git_settings,
        project_git::list_deploy_hooks,
        project_git::create_deploy_hook,
        project_git::delete_deploy_hook,
        project_git::trigger_deploy_hook,
        project_git::restart_project,
        project_git::redeploy_project,
        project_overview::get_project_overview,
        project_overview::list_project_services,
        project_overview::restart_service,
        project_overview::stop_service,
        variables::list_project_variables,
        variables::project_variables_status,
        variables::put_project_variable,
        variables::bulk_project_variables,
        variables::delete_project_variable,
        variables::list_global_variables,
        variables::create_global_variable,
        variables::update_global_variable,
        variables::delete_global_variable,
        variables::export_global_variables,
        domains::list_domains,
        domains::create_domain,
        domains::update_project_domains,
        domains::update_domain,
        domains::delete_domain,
        domains::verify_domain,
        domains::renew_domain,
        domains::primary_domain,
        domains::redirect_domain,
        overview::get_overview,
        panel_domain::get_panel_domain,
        panel_domain::put_panel_domain,
        panel_domain::confirm_panel_domain,
        panel_domain::recheck_panel_domain,
        panel_domain::delete_panel_domain,
        settings::get_general_settings,
        settings::patch_general_settings,
        settings::get_deploy_settings,
        settings::patch_deploy_settings,
        settings::get_org_security,
        settings::patch_org_security,
        settings::get_danger_zone,
        settings::export_organization,
        settings::delete_organization,
        system::get_system_version,
        system::request_update
    ),
    components(schemas(
        health::HealthResponse,
        setup::SetupStatus,
        setup::SetupCheck,
        setup::SetupChecks,
        setup::SetupAdmin,
        setup::SetupOrganization,
        setup::SetupComplete,
        setup::SetupUser,
        setup::SetupCompleted,
        auth::LoginRequest,
        auth::LoginResponse,
        auth::Me,
        mfa::MfaCode,
        mfa::TotpStart,
        mfa::TotpConfirmed,
        mfa::MfaVerified,
        mfa::MfaMethods,
        mfa::DisableTotp,
        mfa::AddFactor,
        webauthn::WebauthnStatus,
        webauthn::WebauthnOptions,
        webauthn::WebauthnRegister,
        webauthn::WebauthnRegistered,
        webauthn::WebauthnCredential,
        webauthn::WebauthnCredentials,
        webauthn::DeleteWebauthn,
        webauthn::AssertMode,
        webauthn::AssertOptionsRequest,
        webauthn::AssertVerify,
        invitations::CreateInvitation,
        invitations::InvitationLink,
        invitations::InvitationInfo,
        invitations::AcceptInvitation,
        invitations::SignupStatus,
        invitations::RegisterRequest,
        team::TeamMember,
        team::UpdateMemberRole,
        team::RemovedMember,
        team::PendingInvitation,
        password::PasswordResetStatus,
        password::ForgotPassword,
        password::ResetTokenInfo,
        password::ResetPassword,
        password::ResetLink,
        notifications::NotificationSettings,
        notifications::UpdateNotificationSettings,
        me::UpdateMe,
        me::AvatarUpdated,
        me::ChangePassword,
        me::SessionInfo,
        me::Sessions,
        me::NotificationPrefs,
        me::NotificationPrefsView,
        projects::GitSource,
        projects::ImageSource,
        projects::Source,
        projects::Build,
        projects::ProjectStatus,
        projects::NewProject,
        projects::UpdateProject,
        projects::ProjectSummary,
        projects::Environment,
        projects::Service,
        projects::Project,
        github::GithubManifestRequest,
        github::GithubManifest,
        github::GithubAppInfo,
        github::GithubStatus,
        github::GithubRepo,
        deployments::DeploymentStatus,
        deployments::DeploymentTrigger,
        deployments::DeploymentStage,
        deployments::StepStatus,
        deployments::DeploymentStep,
        deployments::Deployment,
        deployments::NewDeployment,
        project_git::GitSettings,
        project_git::UpdateGitSettings,
        project_git::DeployHook,
        project_git::NewDeployHook,
        project_git::CreatedDeployHook,
        project_git::HookTriggered,
        project_git::RestartRequested,
        project_overview::ServiceState,
        project_overview::Replicas,
        project_overview::ProjectService,
        project_overview::Sparkline,
        project_overview::OverviewMetrics,
        project_overview::ProjectOverview,
        project_overview::ServiceActionAccepted,
        variables::ProjectVariable,
        variables::VariablesStatus,
        variables::PutProjectVariable,
        variables::BulkProjectVariables,
        variables::GlobalVariable,
        variables::NewGlobalVariable,
        variables::UpdateGlobalVariable,
        domains::DnsStatus,
        domains::CertStatus,
        domains::Domain,
        domains::ProjectDomains,
        domains::NewDomain,
        domains::UpdateDomain,
        domains::UpdateProjectDomains,
        domains::DomainRedirect,
        overview::OverviewPeriod,
        overview::IssueKind,
        overview::OverviewIssue,
        overview::ChartPoint,
        overview::KpiSeries,
        overview::OverviewChart,
        overview::ServerLoad,
        overview::Overview,
        panel_domain::PanelDomainStatus,
        panel_domain::PanelDomain,
        panel_domain::SetPanelDomain,
        panel_domain::ConfirmPanelDomain,
        settings::GeneralSettings,
        settings::UpdateGeneralSettings,
        settings::DeploySettings,
        settings::UpdateDeploySettings,
        settings::OrgSecurity,
        settings::UpdateOrgSecurity,
        settings::DangerZone,
        settings::DeleteOrganization,
        settings::RemainingProject,
        settings::GithubAppNotice,
        settings::OrganizationDeleted,
        system::SystemVersion,
        system::RequestUpdate,
        system::UpdateRequested
    ))
)]
struct ApiDoc;

/// Document OpenAPI complet de l'API du control-plane.
pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_has_health() {
        assert!(openapi().paths.paths.contains_key("/api/health"));
    }

    #[test]
    fn openapi_has_setup_endpoints() {
        let doc = openapi();
        for path in [
            "/api/setup/status",
            "/api/setup/checks",
            "/api/setup/complete",
        ] {
            assert!(doc.paths.paths.contains_key(path), "{path}");
        }
    }

    #[test]
    fn openapi_has_auth_endpoints() {
        let doc = openapi();
        for path in ["/api/auth/login", "/api/auth/logout", "/api/me"] {
            assert!(doc.paths.paths.contains_key(path), "{path}");
        }
    }

    #[test]
    fn openapi_has_mfa_endpoints() {
        let doc = openapi();
        for path in [
            "/api/me/mfa/totp/start",
            "/api/me/mfa/totp/confirm",
            "/api/me/mfa/totp",
            "/api/auth/mfa/verify",
            "/api/auth/recovery-code",
        ] {
            assert!(doc.paths.paths.contains_key(path), "{path}");
        }
    }

    #[test]
    fn operation_ids_are_unique() {
        let doc = openapi();
        let mut ids: Vec<String> = doc
            .paths
            .paths
            .values()
            .flat_map(|item| {
                [&item.get, &item.put, &item.post, &item.delete, &item.patch]
                    .into_iter()
                    .flatten()
                    .filter_map(|op| op.operation_id.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "operationId en double");
    }

    #[test]
    fn openapi_has_webauthn_endpoints() {
        let doc = openapi();
        for path in [
            "/api/auth/mfa/methods",
            "/api/auth/webauthn/status",
            "/api/me/webauthn",
            "/api/me/webauthn/{id}",
            "/api/me/webauthn/register/options",
            "/api/me/webauthn/register/verify",
            "/api/auth/webauthn/assert/options",
            "/api/auth/webauthn/assert/verify",
        ] {
            assert!(doc.paths.paths.contains_key(path), "{path}");
        }
        let pw: webauthn::DeleteWebauthn =
            serde_json::from_str(r#"{"password":"test-password-value"}"#).unwrap();
        assert!(!format!("{pw:?}").contains("test-password-value"));
    }

    #[test]
    fn openapi_has_invitation_endpoints() {
        let doc = openapi();
        for path in [
            "/api/team/invitations",
            "/api/auth/invitations/{token}",
            "/api/auth/invitations/{token}/accept",
            "/api/auth/register",
        ] {
            assert!(doc.paths.paths.contains_key(path), "{path}");
        }
        let accept: invitations::AcceptInvitation =
            serde_json::from_str(r#"{"name":"N","password":"test-password-value"}"#).unwrap();
        assert!(!format!("{accept:?}").contains("test-password-value"));
        let reg: invitations::RegisterRequest = serde_json::from_str(
            r#"{"name":"N","email":"a@example.com","password":"test-password-value"}"#,
        )
        .unwrap();
        let dbg = format!("{reg:?}");
        assert!(!dbg.contains("test-password-value") && !dbg.contains("a@example.com"));
        let link = invitations::InvitationLink {
            link: "/invite/SECRETTESTTOKEN".into(),
        };
        assert!(!format!("{link:?}").contains("SECRETTESTTOKEN"));
    }

    #[test]
    fn openapi_has_team_endpoints() {
        let doc = openapi();
        for path in [
            "/api/team/members",
            "/api/team/members/{id}",
            "/api/team/invitations",
            "/api/team/invitations/{id}",
        ] {
            assert!(doc.paths.paths.contains_key(path), "{path}");
        }
        let item = &doc.paths.paths["/api/team/invitations"];
        assert!(item.get.is_some() && item.post.is_some());
        let item = &doc.paths.paths["/api/team/members/{id}"];
        assert!(item.patch.is_some() && item.delete.is_some());
    }

    #[test]
    fn openapi_has_password_reset_and_notification_endpoints() {
        let doc = openapi();
        for path in [
            "/api/auth/password/forgot",
            "/api/auth/password/reset/{token}",
            "/api/auth/password/reset",
            "/api/team/members/{id}/reset-link",
            "/api/settings/notifications",
        ] {
            assert!(doc.paths.paths.contains_key(path), "{path}");
        }
        let reset: password::ResetPassword =
            serde_json::from_str(r#"{"token":"SECRETTESTTOKEN","password":"test-password-value"}"#)
                .unwrap();
        let dbg = format!("{reset:?}");
        assert!(!dbg.contains("SECRETTESTTOKEN") && !dbg.contains("test-password-value"));
        let forgot: password::ForgotPassword =
            serde_json::from_str(r#"{"email":"a@example.com"}"#).unwrap();
        assert!(!format!("{forgot:?}").contains("a@example.com"));
        let link = password::ResetLink {
            link: "/reset/SECRETTESTTOKEN".into(),
        };
        assert!(!format!("{link:?}").contains("SECRETTESTTOKEN"));
        let smtp: notifications::UpdateNotificationSettings = serde_json::from_str(
            r#"{"enabled":true,"host":"h","port":587,"tls":"starttls","password":"smtp-test-secret","from_address":"a@b.c","from_name":"C"}"#,
        )
        .unwrap();
        assert!(!format!("{smtp:?}").contains("smtp-test-secret"));
        let keep: notifications::UpdateNotificationSettings = serde_json::from_str(
            r#"{"enabled":true,"host":"h","port":587,"tls":"tls","from_address":"a@b.c","from_name":"C"}"#,
        )
        .unwrap();
        assert!(keep.password.is_none() && keep.username.is_empty());
    }

    #[test]
    fn openapi_has_project_endpoints() {
        let doc = openapi();
        let list = &doc.paths.paths["/api/projects"];
        assert!(list.get.is_some() && list.post.is_some());
        let one = &doc.paths.paths["/api/projects/{id}"];
        assert!(one.get.is_some() && one.patch.is_some() && one.delete.is_some());
        let body: projects::NewProject = serde_json::from_str(
            r#"{"name":"a","source":{"type":"git","repository":"o/r","branch":"main"},"build":{"kind":"compose","path":"c.yaml","service":"web"}}"#,
        )
        .unwrap();
        assert!(matches!(body.build, Some(projects::Build::Compose { .. })));
        assert!(
            serde_json::from_str::<projects::NewProject>(
                r#"{"name":"a","x":1,"source":{"type":"image","image":"n"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn openapi_has_deployment_endpoints() {
        let doc = openapi();
        assert!(
            doc.paths.paths["/api/projects/{id}/deployments"]
                .post
                .is_some()
        );
        assert!(doc.paths.paths["/api/deployments"].get.is_some());
        assert!(doc.paths.paths["/api/deployments/{id}"].get.is_some());
        assert!(
            doc.paths.paths["/api/deployments/{id}/cancel"]
                .post
                .is_some()
        );
        assert!(
            doc.paths.paths["/api/deployments/{id}/rollback"]
                .post
                .is_some()
        );
        let empty: deployments::NewDeployment = serde_json::from_str("{}").unwrap();
        assert!(empty.environment.is_none());
        assert!(
            serde_json::from_str::<deployments::NewDeployment>(
                r#"{"environment":"production","x":1}"#
            )
            .is_err()
        );
        for s in deployments::DeploymentStatus::ALL {
            assert_eq!(deployments::DeploymentStatus::parse(s.as_str()), Some(s));
        }
    }

    #[test]
    fn openapi_has_profile_endpoints() {
        let doc = openapi();
        for path in [
            "/api/me",
            "/api/me/avatar",
            "/api/me/password",
            "/api/me/sessions",
            "/api/me/sessions/{id}",
            "/api/me/notification-prefs",
        ] {
            assert!(doc.paths.paths.contains_key(path), "{path}");
        }
        let me = &doc.paths.paths["/api/me"];
        assert!(me.get.is_some() && me.patch.is_some());
        let avatar = &doc.paths.paths["/api/me/avatar"];
        assert!(avatar.get.is_some() && avatar.put.is_some() && avatar.delete.is_some());
        let update: me::UpdateMe = serde_json::from_str(
            r#"{"email":"a@example.com","current_password":"test-password-value"}"#,
        )
        .unwrap();
        let dbg = format!("{update:?}");
        assert!(!dbg.contains("test-password-value") && !dbg.contains("a@example.com"));
        let change: me::ChangePassword = serde_json::from_str(
            r#"{"current_password":"old-test-password","new_password":"new-test-password"}"#,
        )
        .unwrap();
        let dbg = format!("{change:?}");
        assert!(!dbg.contains("old-test-password") && !dbg.contains("new-test-password"));
        assert!(serde_json::from_str::<me::UpdateMe>(r#"{"role":"owner"}"#).is_err());
    }

    #[test]
    fn openapi_has_variable_endpoints() {
        let doc = openapi();
        let project = &doc.paths.paths["/api/projects/{id}/variables"];
        assert!(project.get.is_some() && project.put.is_some());
        assert!(
            doc.paths.paths["/api/projects/{id}/variables/bulk"]
                .put
                .is_some()
        );
        assert!(
            doc.paths.paths["/api/projects/{id}/variables/{key}"]
                .delete
                .is_some()
        );
        assert!(
            doc.paths.paths["/api/projects/{id}/variables/status"]
                .get
                .is_some()
        );
        let globals = &doc.paths.paths["/api/variables"];
        assert!(globals.get.is_some() && globals.post.is_some());
        let one = &doc.paths.paths["/api/variables/{id}"];
        assert!(one.patch.is_some() && one.delete.is_some());
        assert!(doc.paths.paths["/api/variables/export"].get.is_some());
        let put: variables::PutProjectVariable =
            serde_json::from_str(r#"{"key":"K","value":"secret-value-x","secret":true}"#).unwrap();
        assert!(!format!("{put:?}").contains("secret-value-x"));
        let bulk: variables::BulkProjectVariables =
            serde_json::from_str(r#"{"content":"K=secret-value-x"}"#).unwrap();
        assert!(!format!("{bulk:?}").contains("secret-value-x"));
        let new: variables::NewGlobalVariable = serde_json::from_str(
            r#"{"key":"K","value":"secret-value-x","secret":true,"environments":["production"],"all_projects":true}"#,
        )
        .unwrap();
        assert!(!format!("{new:?}").contains("secret-value-x"));
        let patch: variables::UpdateGlobalVariable =
            serde_json::from_str(r#"{"value":"secret-value-x"}"#).unwrap();
        assert!(!format!("{patch:?}").contains("secret-value-x"));
    }

    #[test]
    fn openapi_has_settings_endpoints() {
        let doc = openapi();
        for path in [
            "/api/settings/general",
            "/api/settings/deployments",
            "/api/settings/security",
        ] {
            let item = &doc.paths.paths[path];
            assert!(item.get.is_some() && item.patch.is_some(), "{path}");
        }
        assert!(doc.paths.paths["/api/settings/danger"].get.is_some());
        assert!(doc.paths.paths["/api/organization/export"].post.is_some());
        assert!(doc.paths.paths["/api/organization"].delete.is_some());
        let del: settings::DeleteOrganization =
            serde_json::from_str(r#"{"password":"test-password-value","confirm":"SUPPRIMER"}"#)
                .unwrap();
        assert!(!format!("{del:?}").contains("test-password-value"));
        assert!(serde_json::from_str::<settings::UpdateOrgSecurity>(r#"{"x":1}"#).is_err());
    }

    #[test]
    fn mfa_bodies_debug_hide_secrets() {
        let code: mfa::MfaCode = serde_json::from_str(r#"{"code":"123456"}"#).unwrap();
        assert!(!format!("{code:?}").contains("123456"));
        let pw: mfa::DisableTotp =
            serde_json::from_str(r#"{"password":"test-password-value"}"#).unwrap();
        assert!(!format!("{pw:?}").contains("test-password-value"));
        let start = mfa::TotpStart {
            otpauth_uri: "otpauth://totp/x?secret=ABCDEF".into(),
            qr_svg: "<svg/>".into(),
        };
        assert!(!format!("{start:?}").contains("ABCDEF"));
        let confirmed = mfa::TotpConfirmed {
            recovery_codes: vec!["AAAAA-BBBBB".into()],
            csrf_token: "t".into(),
        };
        assert!(!format!("{confirmed:?}").contains("AAAAA"));
    }

    #[test]
    fn login_request_debug_hides_password_and_email() {
        let req: auth::LoginRequest =
            serde_json::from_str(r#"{"email":"a@example.com","password":"test-password-value"}"#)
                .unwrap();
        assert!(!req.remember, "remember vaut false par defaut");
        let dbg = format!("{req:?}");
        assert!(!dbg.contains("test-password-value"));
        assert!(!dbg.contains("a@example.com"));
    }

    #[test]
    fn setup_admin_debug_hides_password() {
        let admin = setup::SetupAdmin {
            name: "A".into(),
            email: "a@example.com".into(),
            password: "test-password-value".into(),
        };
        assert!(!format!("{admin:?}").contains("test-password-value"));
    }
}
