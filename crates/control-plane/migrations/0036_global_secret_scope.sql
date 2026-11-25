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
-- Le controle de recoupement des globales (meme cle, environnements communs,
-- projet commun) n'existait pas pour les anciennes donnees : une globale plus
-- precise l'emportait deja sur le secret (moins d'environnements, ou portee
-- par projets, ou plus recente). L'expansion n'ajoute donc pas un projet a un
-- secret qui perdait deja pour lui : l'ordre de priorite, et donc les valeurs
-- injectees, ne changent pas. Un recoupement ne subsiste que si le secret
-- l'emportait deja (il existait deja, le controle de recoupement le refusera
-- a la prochaine modification).
-- L'ordre est celui de `global_rows` : nombre d'environnements croissant, puis
-- portee par projets avant "tous", puis la plus recente, puis l'identifiant.
INSERT INTO global_variable_projects (variable_id, project_id)
SELECT t.id, p.id
FROM global_variables t
CROSS JOIN projects p
WHERE t.secret AND t.all_projects
  AND NOT EXISTS (
      SELECT 1
      FROM global_variables o
      WHERE o.id <> t.id
        AND o.key = t.key
        AND o.environments && t.environments
        AND (o.all_projects
             OR EXISTS (SELECT 1 FROM global_variable_projects op
                        WHERE op.variable_id = o.id AND op.project_id = p.id))
        AND (cardinality(o.environments) < cardinality(t.environments)
             OR (cardinality(o.environments) = cardinality(t.environments)
                 AND (NOT o.all_projects
                      OR o.updated_at > t.updated_at
                      OR (o.updated_at = t.updated_at AND o.id < t.id))))
  );

UPDATE global_variables SET all_projects = false WHERE secret AND all_projects;

ALTER TABLE global_variables
    ADD CONSTRAINT global_variables_secret_scope_check
    CHECK (NOT (secret AND all_projects));
