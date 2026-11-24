-- Service arrete depuis le panneau : deploiement servi dont les conteneurs
-- ont ete arretes. Le service est « arrete » tant que ce deploiement reste
-- le deploiement servi ; un redemarrage efface la marque, et un nouveau
-- deploiement reussi la rend caduque (il demarre de nouveau).
ALTER TABLE services
    ADD COLUMN stopped_deployment uuid REFERENCES deployments (id) ON DELETE SET NULL;
