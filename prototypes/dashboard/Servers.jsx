(()=>{
const {StatGroup,Stat,DataTable,StatusDot,Tabs,Button,Badge,CodeBlock,KeyValueList,Modal,TextField,RadioCard,Icon,AreaChart,RadialGauge,TickBar,LegendItem}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const mono=v=><span style={{fontFamily:'var(--font-mono)',fontSize:12}}>{v}</span>;
const dot=v=><StatusDot status={v[0]} label={v[1]}/>;
function ServersList({open}){
  const D=window.CIPHERA;
  return <div>
    <div style={{padding:'16px 32px 28px 46px',borderBottom:hair}}><StatGroup><Stat label="Agents connectés" value="3 / 4"/><Stat label="CPU moyen" value="51%" delta="4.2%" direction="down" deltaSuffix="vs 7 jours précédents"/><Stat label="RAM des agents" value="17 Mo" deltaSuffix="en moyenne par nœud" delta="8.0%"/></StatGroup></div>
    <div style={{padding:'28px 32px 40px 34px'}}>
      <DataTable onRowClick={r=>open(r.id)} columns={[{key:'name',label:'Serveur'},{key:'region',label:'Région',width:'1.3fr'},{key:'status',label:'Statut',render:dot},{key:'cpu',label:'CPU',render:v=>v+' %'},{key:'ram',label:'RAM'},{key:'containers',label:'Conteneurs'},{key:'agent',label:'Agent',align:'right',render:v=>mono('v'+v)}]} rows={D.servers}/>
    </div>
  </div>;
}
function ServerDetail({id,back}){
  const D=window.CIPHERA,S=D.servers.find(s=>s.id===id)||D.servers[0];
  const [tab,setTab]=React.useState('Métriques');
  const cpu=D.latency.map(v=>Math.min(100,Math.max(0,S.cpu+(v-155)/2)));
  return <div>
    <div style={{padding:'20px 32px 0 46px',display:'flex',flexDirection:'column',gap:20}}>
      <button onClick={back} style={{alignSelf:'flex-start',display:'flex',alignItems:'center',gap:6,background:'none',border:0,padding:0,color:'var(--text-subtle)',font:'400 12px/1 var(--font-sans)',cursor:'pointer'}}><Icon name="arrow-left" size={13}/>Serveurs</button>
      <div style={{display:'flex',alignItems:'flex-end',justifyContent:'space-between',gap:24}}>
        <div style={{display:'flex',flexDirection:'column',gap:10}}>
          <div style={{display:'flex',alignItems:'center',gap:12}}><span style={{font:'500 22px/1.1 var(--font-sans)',color:'var(--text-strong)'}}>{S.name}</span><Badge>{'Agent v'+S.agent}</Badge></div>
          <div style={{display:'flex',gap:18,alignItems:'center',font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>{dot(S.status)}<span>{S.region}</span>{mono(S.ip)}</div>
        </div>
        <div style={{display:'flex',gap:8}}><Button icon="refresh-cw">Mettre à jour l'agent</Button><Button icon="key-round">Renouveler le certificat</Button></div>
      </div>
      <Tabs tabs={['Métriques',{value:'Conteneurs',label:'Conteneurs',count:S.containers},'Agent']} value={tab} onChange={setTab}/>
    </div>
    {tab==='Métriques'&&<div style={{display:'flex'}}>
      <div style={{flex:1,minWidth:0,padding:'28px 32px 32px 46px'}}>
        <div style={{marginBottom:36}}><Stat size="lg" labelBelow label="Utilisation CPU (%)" value={S.cpu+' %'}/></div>
        <AreaChart data={cpu} yMax={100} yTicks={[100,75,50,25,0]} yFormat={v=>String(v)} series="CPU" valueFormat={v=>Math.round(v)+' %'} pointLabels={D.days} tooltipLabels={D.long} ticks={[0,6,11,17,23,29]} height={170}/>
      </div>
      <aside style={{width:'var(--rail-width)',flex:'none',borderLeft:hair,padding:'22px 28px 28px',display:'flex',flexDirection:'column',alignItems:'center',gap:18}}>
        <RadialGauge value={S.cpu?0.76:0} size={230} icon="memory-stick" label="Mémoire" amount={S.ram}/>
        <div style={{alignSelf:'stretch'}}><div style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)',marginBottom:4}}>Disque · {S.disk}</div><TickBar value={parseInt(S.disk)/parseInt(S.disk.split('/ ')[1])} ticks={80}/></div>
      </aside>
    </div>}
    {tab==='Conteneurs'&&<div style={{padding:'24px 32px 40px 34px'}}><DataTable columns={[{key:'n',label:'Conteneur',render:mono},{key:'p',label:'Projet'},{key:'s',label:'Statut',render:dot},{key:'c',label:'CPU'},{key:'m',label:'Mémoire'},{key:'u',label:'Uptime',align:'right'}]} rows={[{n:'web-4821-a',p:'ciphera-web',s:['ok','Sain'],c:'12 %',m:'312 Mo',u:'2 min'},{n:'web-4821-b',p:'ciphera-web',s:['ok','Sain'],c:'10 %',m:'298 Mo',u:'2 min'},{n:'worker-4818',p:'ciphera-web',s:['ok','Sain'],c:'4 %',m:'141 Mo',u:'4 h'},{n:'postgres-17',p:'ciphera-web',s:['ok','Sain'],c:'6 %',m:'1,1 Go',u:'21 j'},{n:'auth-api-a',p:'auth-service',s:['pending','Redémarré'],c:'3 %',m:'48 Mo',u:'12 min'}]}/></div>}
    {tab==='Agent'&&<div style={{padding:'28px 32px 40px 46px',display:'grid',gridTemplateColumns:'minmax(0,420px) minmax(0,1fr)',gap:40}}>
      <KeyValueList title="Agent nœud" rows={[{label:'Version :',value:'v'+S.agent},{label:'Transport :',value:'QUIC · mTLS'},{label:'Latence plan de contrôle :',value:S.rtt},{label:'Certificat :',value:S.cert},{label:'Système :',value:S.os+' · '+S.cores+' vCPU'},{label:'API exposée :',value:<Badge>deploy · stop · logs · metrics · pull</Badge>}]}/>
      <div style={{display:'flex',flexDirection:'column',gap:12}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}>Journal de l'agent</span>
        <CodeBlock maxHeight={260} lines={[{time:'12:04:31',text:'quic: session reprise (0-RTT)',tone:'muted'},{time:'12:04:32',text:'deploy(spec=ciphera-web#4821) reçu',tone:'info'},{time:'12:04:49',text:'pull registry.ciphera.dev/web:4821 ok',tone:'info'},{time:'12:04:52',text:'2 conteneurs sains, ancien set arrêté',tone:'ok'},{time:'12:10:00',text:'metrics: cpu=38% ram=5.2G disk=61G',tone:'muted'}]}/>
      </div>
    </div>}
  </div>;
}
function AddServerModal({onClose}){
  const [os,setOs]=React.useState('linux');
  return <Modal title="Ajouter un serveur" description="Installez l'agent Ciphera sur votre machine. Il se connecte au plan de contrôle en QUIC avec un certificat mTLS dédié." onClose={onClose} width={580} footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" onClick={onClose}>J'ai lancé la commande</Button></>}>
    <TextField label="Nom du serveur" placeholder="srv-par-03"/>
    <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:8}}>
      <RadioCard icon="terminal" title="Script d'installation" description="Debian, Ubuntu, RHEL — systemd" selected={os==='linux'} onClick={()=>setOs('linux')}/>
      <RadioCard icon="container" title="Conteneur Docker" description="Agent lancé avec bollard, sans shell" selected={os==='docker'} onClick={()=>setOs('docker')}/>
    </div>
    <div style={{display:'flex',flexDirection:'column',gap:8}}>
      <span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>Commande à exécuter sur le serveur</span>
      <CodeBlock code={os==='linux'?'curl -fsSL https://get.ciphera.dev | sh -s -- \\\n  --token cph_join_7f3a••••••••92c1':'docker run -d --name ciphera-agent \\\n  -v /var/run/docker.sock:/var/run/docker.sock \\\n  ghcr.io/ciphera/agent:0.9.4 --token cph_join_7f3a••••••••92c1'}/>
      <span style={{display:'flex',alignItems:'center',gap:8,font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}><StatusDot status="pending"/>En attente de la connexion de l'agent… · le jeton expire dans 15 min</span>
    </div>
  </Modal>;
}
Object.assign(window,{ServersList,ServerDetail,AddServerModal});
})();
