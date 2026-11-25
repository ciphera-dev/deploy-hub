-- Portee explicite des secrets globaux.
--
-- Un secret global ne vise plus "tous les projets" : un projet cree plus tard
-- heriterait d'un secret sans que personne l'ait decide. Les secrets deja
-- definis pour tous les projets recoivent la liste explicite des projets
-- actuels, puis la base interdit la combinaison (`secret` et `all_projects`).
-- Les publiques gardent `all_projects`.
--
-- Cette migration porte le numero 0036 mais est livree apres 0037 : sqlx
-- applique les migrations non encore appliquees quel que soit leur ordre.
--
-- Les valeurs effectives de chaque (projet, environnement) ne changent pas.
-- La priorite de `global_rows` se calcule par couple (projet, environnement) :
-- nombre d'environnements croissant, puis portee par projets avant "tous", puis
-- la plus recente, puis l'identifiant. Un projet n'est donc laisse hors de la
-- liste d'un secret que si le secret y est entierement masque : chacun de ses
-- environnements est couvert, pour ce projet, par une autre globale de meme cle
-- qui l'emporte deja sur lui. Pour tout autre projet (y compris quand une
-- globale plus precise ne couvre qu'une partie des environnements), le secret
-- garde sa place : il est le seul fournisseur dans les autres environnements.
--
-- Les recoupements ainsi conserves existaient deja (le controle de recoupement
-- date apres ces donnees). Ils ne sont pas corriges ici : le controle des
-- globales est execute a chaque modification, y compris la rotation de la
-- valeur du secret, et la refusera (422 variable_scope_overlap) tant que la
-- portee de l'une des deux lignes n'est pas corrigee.
--
-- Limite connue : quand un secret garde un projet que partage une globale de
-- meme cle, de meme nombre d'environnements mais d'environnements differents,
-- et qui ne l'emportait que par le depart "tous" ou la date, le passage a une
-- portee par projets peut changer lequel des deux gagne dans leurs
-- environnements communs. Cette configuration est un recoupement refuse depuis
-- R36 ; aucune donnee v0.1 ne l'a produite hors tests.
-- Un secret peut finir sans aucun projet (aucun projet, ou masque partout) : il
-- n'est alors injecte nulle part ; l'administrateur lui choisit ses projets.
INSERT INTO global_variable_projects (variable_id, project_id)
SELECT t.id, p.id
FROM global_variables t
CROSS JOIN projects p
WHERE t.secret AND t.all_projects
  AND EXISTS (
      SELECT 1
      FROM unnest(t.environments) AS e (name)
      WHERE NOT EXISTS (
          SELECT 1
          FROM global_variables o
          WHERE o.id <> t.id
            AND o.key = t.key
            AND e.name = ANY (o.environments)
            AND (o.all_projects
                 OR EXISTS (SELECT 1 FROM global_variable_projects op
                            WHERE op.variable_id = o.id AND op.project_id = p.id))
            AND (cardinality(o.environments) < cardinality(t.environments)
                 OR (cardinality(o.environments) = cardinality(t.environments)
                     AND (NOT o.all_projects
                          OR o.updated_at > t.updated_at
                          OR (o.updated_at = t.updated_at AND o.id < t.id))))
      )
  )
ON CONFLICT DO NOTHING;

UPDATE global_variables SET all_projects = false WHERE secret AND all_projects;

ALTER TABLE global_variables
    ADD CONSTRAINT global_variables_secret_scope_check
    CHECK (NOT (secret AND all_projects));
