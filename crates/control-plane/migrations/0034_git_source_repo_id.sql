-- Depot identifie par son id GitHub (stable meme si le nom est renomme ou
-- repris par un autre compte), et projet cree via la GitHub App : pour ce
-- dernier, plus de repli sur un clone anonyme.
ALTER TABLE git_sources
    ADD COLUMN github_repo_id bigint,
    ADD COLUMN via_app boolean NOT NULL DEFAULT false;
