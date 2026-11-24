(()=>{
const {StatGroup,Stat,AreaChart,Delta,Panel,InsightText,Button,SegmentedBar,RadialGauge,LegendItem,TickBar,KeyValueList,Badge,StatusDot}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
function Overview({p,go}){
  const D=window.CIPHERA,L=['Déploiements','Taux de succès','Durée moyenne de build'];
  const last=D.deployments[0];
  return <div style={{display:'flex',flex:1,minHeight:0,borderTop:hair}}>
    <div style={{flex:1,minWidth:0,display:'flex',flexDirection:'column'}}>
      <div style={{padding:'16px 32px 28px 46px',borderBottom:hair}}>
        <StatGroup>{p.kpi.map((k,i)=><Stat key={i} label={L[i]} value={k[0]} delta={k[1]} direction={k[2]} deltaSuffix="vs période précédente"/>)}</StatGroup>
      </div>
      <div style={{padding:'28px 32px 26px 46px',borderBottom:hair}}>
        <div style={{display:'flex',alignItems:'flex-end',justifyContent:'space-between',marginBottom:36}}>
          <Stat size="lg" labelBelow label="Déploiements par jour" value={p.dep}/>
          <Delta value={p.depDelta} suffix="sur les 30 derniers jours"/>
        </div>
        <AreaChart data={D.deploys} yMax={100} yTicks={[100,80,60,40,20,0]} yFormat={v=>String(v)} series="Déploiements" valueFormat={v=>String(v)} pointLabels={D.days} tooltipLabels={D.long} ticks={[0,6,11,17,23,29]} height={176}/>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) minmax(0,1fr)'}}>
        <Panel padding="40px 32px 40px 46px" icon="sparkles" title="Insights" action={<Button variant="ghost" icon="message-circle">Demander à l'IA</Button>} style={{gap:40,borderRight:hair}}>
          <InsightText highlight="38 % de cache en plus" after="depuis le passage à BuildKit.">Les builds sont plus rapides de 11 s grâce à</InsightText>
        </Panel>
        <Panel padding="36px 32px 40px 32px" style={{gap:30}} title={<div style={{display:'flex',flexDirection:'column',gap:10}}><span>Stockage du cluster</span><span style={{font:'500 22px/1 var(--font-sans)',color:'var(--text-strong)'}}>405 / 720 Go</span></div>} action={<Button variant="ghost" icon="hard-drive" style={{alignSelf:'flex-start'}} onClick={()=>go('Serveurs')}>Gérer</Button>}>
          <SegmentedBar segments={[{value:44,label:'44%',name:'Images',color:'var(--viz-1)'},{value:22,label:'22%',name:'Volumes',color:'var(--viz-2)'},{value:34,label:'34%',name:'Libre',color:'var(--viz-3)'}]}/>
        </Panel>
      </div>
    </div>
    <aside style={{width:'var(--rail-width)',flex:'none',borderLeft:hair,display:'flex',flexDirection:'column'}}>
      <div style={{padding:'22px 28px 28px',borderBottom:hair,display:'flex',flexDirection:'column',alignItems:'center',gap:18}}>
        <RadialGauge value={p.cpu} size={264} icon="cpu" label="CPU du cluster" amount={Math.round(p.cpu*100)+' % utilisé'}/>
        <div style={{display:'flex',gap:18}}><LegendItem label="Utilisé"/><LegendItem color="var(--viz-4)" label="Disponible"/></div>
        <Button variant="filled" size="sm" block iconRight="arrow-right" onClick={()=>go('Monitoring')}>Voir le monitoring</Button>
      </div>
      <div style={{padding:'24px 28px 28px',borderBottom:hair}}>
        <div style={{borderLeft:'1.5px solid var(--viz-1)',paddingLeft:10,display:'flex',flexDirection:'column',gap:6}}>
          <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Conteneurs actifs</span>
          <span style={{font:'500 22px/1.1 var(--font-sans)',color:'var(--text-strong)'}}>{p.containers}</span>
        </div>
        <TickBar value={p.online} ticks={92} style={{marginTop:-6}}/>
        <div style={{display:'flex',gap:18,marginTop:12}}><LegendItem label="Sains"/><LegendItem color="var(--viz-4)" label="Redémarrés"/></div>
      </div>
      <div style={{padding:'28px',display:'flex',flexDirection:'column',gap:26}}>
        <KeyValueList title="Dernier déploiement" rows={[{label:'Projet :',value:'ciphera-web'},{label:'Commit :',value:<span style={{fontFamily:'var(--font-mono)'}}>{last.commit}</span>},{label:'Serveur :',value:'srv-par-01'},{label:'Statut :',value:<Badge>{last.s[1]}</Badge>}]}/>
        <Button variant="filled" size="sm" block iconRight="arrow-right" onClick={()=>go('Projets','web')}>Voir le projet</Button>
      </div>
    </aside>
  </div>;
}
window.Overview=Overview;
})();
