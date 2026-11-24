(()=>{
const {Button,IconButton,Icon,Modal,Switch}=window.CipheraDesignSystem;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const hair='1px solid var(--border-subtle)';
const pill=(t,tone)=><span style={{display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:tone==='ok'?'rgba(34,197,94,.12)':tone==='bad'?'rgba(248,113,113,.12)':tone==='warn'?'rgba(229,180,84,.12)':'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:tone==='ok'?'var(--green-500)':tone==='bad'?'#f87171':tone==='warn'?'#e5b454':'var(--text-body)'}}>{t}</span>;
const sep=<span style={{width:1,height:14,background:'var(--border-strong)'}}></span>;
const Head=({ic,title,badge,tone,sub,children})=><div style={{display:'flex',alignItems:'center',gap:16}}>
  <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18)'}}><Icon name={ic} size={24} color="#f2f2f2"/></span>
  <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:tone==='ok'?'rgba(34,197,94,.12)':tone==='bad'?'rgba(248,113,113,.12)':'rgba(229,180,84,.12)',font:'500 12px/1 var(--font-sans)',color:tone==='ok'?'var(--green-500)':tone==='bad'?'#f87171':'#e5b454'}}>{badge}</span></span><span style={lbl}>{sub}</span></span>
  {children}
</div>;
const Back=({s,back})=><button onClick={back} style={{display:'flex',alignItems:'center',gap:6,alignSelf:'flex-start',padding:0,border:0,background:'transparent',cursor:'pointer',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="arrow-left" size={14}/>{s.name}</button>;
const Primary=({ic,children,onClick,busy})=><button onClick={onClick} disabled={busy} style={{display:'flex',alignItems:'center',gap:7,height:38,padding:'0 16px',borderRadius:10,border:0,cursor:busy?'default':'pointer',background:'#8b7cf6',color:'#fff',font:'600 13px/1 var(--font-sans)',opacity:busy?.7:1}}><Icon name={busy?'loader':ic} size={14} color="#fff" style={busy?{animation:'spin 1s linear infinite'}:undefined}/>{children}</button>;

/* ---------- Pare-feu ---------- */
const FW0=[
  {id:1,act:'allow',proto:'TCP',port:'22',src:'Mes IP (3)',note:'SSH',sys:true,dir:'in',ip:'v4'},
  {id:2,act:'allow',proto:'TCP',port:'80',src:'Partout',note:'HTTP',dir:'in',ip:'both'},
  {id:3,act:'allow',proto:'TCP',port:'443',src:'Partout',note:'HTTPS',dir:'in',ip:'both'},
  {id:4,act:'deny',proto:'TCP',port:'5432',src:'Partout',note:'PostgreSQL public',dir:'in',ip:'both'},
  {id:5,act:'deny',proto:'TCP',port:'25',src:'Partout',note:'SMTP sortant',dir:'out',ip:'both'},
  {id:6,act:'allow',proto:'UDP',port:'51820',src:'Partout',note:'WireGuard',dir:'both',ip:'v6'},
];
function RuleModal({onClose,onSave}){
  const [r,setR]=React.useState({act:'allow',proto:'TCP',port:'',src:'Partout',ip:'',note:'',dir:'in',ipv:'both'});
  const PRE=[['SSH','TCP','22'],['HTTP','TCP','80'],['HTTPS','TCP','443'],['PostgreSQL','TCP','5432'],['MySQL','TCP','3306'],['Redis','TCP','6379']];
  const seg=(k,opts)=><div style={{display:'flex',gap:2,padding:3,borderRadius:10,border:'1px solid var(--border-default)'}}>{opts.map(([v,t])=><button key={v} onClick={()=>setR({...r,[k]:v})} style={{flex:1,height:30,padding:'0 12px',borderRadius:7,border:0,cursor:'pointer',font:'500 12px/1 var(--font-sans)',background:r[k]===v?'var(--surface-selected)':'transparent',color:r[k]===v?'var(--text-strong)':'var(--text-muted)'}}>{t}</button>)}</div>;
  const inp=(k,ph)=><input value={r[k]} onChange={e=>setR({...r,[k]:e.target.value})} placeholder={ph} style={{height:38,padding:'0 11px',borderRadius:9,border:'1px solid var(--border-default)',background:'var(--surface-raised)',color:'var(--text-strong)',font:'400 13px/1 var(--font-mono)',outline:'none',width:'100%',boxSizing:'border-box'}}/>;
  const L=t=><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span>;
  const ok=/^\d{1,5}(-\d{1,5})?$/.test(r.port)&&(r.src!=='ip'||/^[\d.:\/a-f]+$/i.test(r.ip));
  return <Modal title="Nouvelle règle de pare-feu" description="La règle est appliquée immédiatement par l'agent (nftables)." onClose={onClose} width={540}
    footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={!ok} onClick={()=>onSave({...r,src:r.src==='ip'?r.ip:r.src,ip:r.ipv})}>Ajouter la règle</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:16}}>
      <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Modèles')}<div style={{display:'flex',gap:6,flexWrap:'wrap'}}>{PRE.map(([n,p,po])=><button key={n} onClick={()=>setR({...r,proto:p,port:po,note:n})} style={{height:28,padding:'0 10px',borderRadius:99,cursor:'pointer',border:'1px solid '+(r.port===po?'var(--border-strong)':'var(--border-subtle)'),background:r.port===po?'var(--surface-raised)':'transparent',font:'500 12px/1 var(--font-sans)',color:'var(--text-body)'}}>{n} <span style={{fontFamily:'var(--font-mono)',color:'var(--text-subtle)'}}>{po}</span></button>)}</div></div>
      <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:12}}>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Action')}{seg('act',[['allow','Autoriser'],['deny','Bloquer']])}</div>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Protocole')}{seg('proto',[['TCP','TCP'],['UDP','UDP']])}</div>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'1.3fr 1fr',gap:12}}>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Direction')}{seg('dir',[['in','Entrant'],['out','Sortant'],['both','Les deux']])}</div>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Version IP')}{seg('ipv',[['v4','IPv4'],['v6','IPv6'],['both','Les deux']])}</div>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'1fr 1.4fr',gap:12}}>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Port ou plage')}{inp('port','443 ou 8000-8100')}</div>
        <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Description')}{inp('note','Facultatif')}</div>
      </div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}>{L('Source')}{seg('src',[['Partout','Partout'],['Mes IP (3)','Mes IP'],['ip','IP ou plage']])}{r.src==='ip'&&inp('ip','203.0.113.0/24')}</div>
    </div>
  </Modal>;
}
function FirewallView({s,flash,back}){
  const [rules,setRules]=React.useState(FW0);const [on,setOn]=React.useState(true);const [pol,setPol]=React.useState('deny');const [add,setAdd]=React.useState(false);const [del,setDel]=React.useState(null);const [h,setH]=React.useState(null);
  const COLS='minmax(0,1.3fr) 110px 100px 70px minmax(0,1fr) 110px 70px 50px';
  const DIR={in:['arrow-down-to-line','Entrant'],out:['arrow-up-from-line','Sortant'],both:['arrow-down-up','Entrant + sortant']};
  const ipBadges=v=><span style={{display:'flex',gap:4}}>{['v4','v6'].map(x=>{const on=v===x||v==='both';return <span key={x} style={{height:20,display:'flex',alignItems:'center',padding:'0 6px',borderRadius:5,border:'1px solid '+(on?'var(--border-strong)':'var(--border-subtle)'),background:on?'var(--surface-selected)':'transparent',font:'500 11px/1 var(--font-mono)',color:on?'var(--text-strong)':'var(--text-faint)',textDecoration:on?'none':'line-through'}}>IP{x}</span>})}</span>;
  return <div style={{display:'flex',flexDirection:'column',gap:14,paddingTop:4}}>
    <Back s={s} back={back}/>
    <div style={{...card,borderRadius:20,padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
      <Head ic="brick-wall" title="Pare-feu" badge={on?'Actif':'Désactivé'} tone={on?'ok':'bad'} sub={s.name+' · '+rules.length+' règles · nftables'}><Primary ic="plus" onClick={()=>setAdd(true)}>Ajouter une règle</Primary></Head>
      <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
        <span style={{display:'flex',alignItems:'center',gap:10}}><Switch checked={on} onChange={v=>{setOn(v);flash(v?'Pare-feu activé':'Pare-feu désactivé')}}/><span style={{fontWeight:500,color:'var(--text-strong)'}}>Pare-feu</span></span>{sep}
        <span style={{display:'flex',alignItems:'center',gap:8}}>Trafic entrant par défaut :<span style={{display:'flex',gap:2,padding:2,borderRadius:8,border:'1px solid var(--border-default)'}}>{[['deny','Bloquer'],['allow','Autoriser']].map(([k,t])=><button key={k} onClick={()=>{setPol(k);flash('Politique par défaut : '+t.toLowerCase())}} style={{height:24,padding:'0 10px',borderRadius:6,border:0,cursor:'pointer',font:'500 12px/1 var(--font-sans)',background:pol===k?'var(--surface-selected)':'transparent',color:pol===k?'var(--text-strong)':'var(--text-muted)'}}>{t}</button>)}</span></span>{sep}
        <span>Sortant : <span style={{fontWeight:600,color:'var(--text-strong)'}}>tout autorisé</span></span>
        <span style={{flex:1}}></span>
        <span style={lbl}>Paquets bloqués (24 h) : <span style={{fontFamily:'var(--font-mono)',color:'var(--text-strong)'}}>18 412</span></span>
      </div>
    </div>
    <div style={{...card,opacity:on?1:.5,transition:'opacity .2s'}}>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:16,padding:'14px 16px',font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}><span>Règle</span><span>Direction</span><span>Action</span><span>Port</span><span>Source</span><span>IP</span><span>Proto</span><span></span></div>
      {rules.map(r=><div key={r.id} onMouseEnter={()=>setH(r.id)} onMouseLeave={()=>setH(null)} style={{display:'grid',gridTemplateColumns:COLS,gap:16,alignItems:'center',padding:'13px 16px',borderTop:hair,background:h===r.id?'var(--surface-raised)':'transparent'}}>
        <span style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}><span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={r.act==='allow'?DIR[r.dir||'in'][0]:'ban'} size={17} color={r.act==='allow'?'var(--text-strong)':'#f87171'}/></span><span style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{r.note||'Port '+r.port}</span><span style={lbl}>{r.sys?'Requis par Ciphera':'Règle personnalisée'}</span></span></span>
        <span style={{display:'flex',alignItems:'center',gap:6,font:'400 12px/1.2 var(--font-sans)',color:'var(--text-body)'}}><Icon name={DIR[r.dir||'in'][0]} size={13} color="var(--text-muted)"/>{DIR[r.dir||'in'][1]}</span>
        <span style={{display:'flex',alignItems:'center',gap:7}}><Icon name={r.act==='allow'?'circle-check':'circle-x'} size={15} color={r.act==='allow'?'var(--green-500)':'#f87171'}/><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{r.act==='allow'?'Autoriser':'Bloquer'}</span></span>
        <span>{pill(r.port)}</span>
        <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{r.src}</span>
        {ipBadges(r.ip||'v4')}
        <span style={lbl}>{r.proto}</span>
        <span style={{display:'flex',justifyContent:'flex-end',gap:4}}>{!r.sys&&<IconButton size={28} icon="trash-2" label="Supprimer" onClick={()=>setDel(r)}/>}{r.sys&&<Icon name="lock" size={13} color="var(--text-subtle)"/>}</span>
      </div>)}
    </div>
    {add&&<RuleModal onClose={()=>setAdd(false)} onSave={r=>{setRules(l=>[...l,{...r,id:Date.now()}]);setAdd(false);flash('Règle ajoutée · port '+r.port+' · '+({v4:'IPv4',v6:'IPv6',both:'IPv4 + IPv6'}[r.ip]))}}/>}
    {del&&<Modal title="Supprimer cette règle ?" description={'Le port '+del.port+' ('+del.proto+', '+({in:'entrant',out:'sortant',both:'entrant et sortant'}[del.dir||'in'])+', '+({v4:'IPv4',v6:'IPv6',both:'IPv4 et IPv6'}[del.ip||'v4'])+') ne sera plus '+(del.act==='allow'?'ouvert':'bloqué')+' pour '+del.src+'.'} onClose={()=>setDel(null)} width={440} footer={<><Button onClick={()=>setDel(null)}>Annuler</Button><Button variant="inverse" icon="trash-2" onClick={()=>{setRules(l=>l.filter(x=>x.id!==del.id));setDel(null);flash('Règle supprimée')}}>Supprimer</Button></>}></Modal>}
  </div>;
}

/* ---------- Scanner de malwares ---------- */
const PATHS=['/var/www','/home','/opt/ciphera','/etc','/tmp','/var/lib/docker/volumes','/usr/local/bin','/root'];
const SCANS0=[
  {id:1,when:"aujourd'hui · 04:00",kind:'Complète',files:'184 302',dur:'6 min 12 s',found:0},
  {id:2,when:'hier · 04:00',kind:'Complète',files:'184 118',dur:'6 min 05 s',found:1},
  {id:3,when:'il y a 2 j · 04:00',kind:'Complète',files:'183 990',dur:'6 min 01 s',found:0},
];
const THREATS0=[{id:1,file:'/tmp/.x/kdevtmpfsi',sig:'Linux.Trojan.Miner',sev:'Élevée',st:'quarantine',when:'hier · 04:03'}];
function MalwareView({s,flash,back}){
  const [on,setOn]=React.useState(true);const [rt,setRt]=React.useState(true);const [scans,setScans]=React.useState(SCANS0);const [threats,setThreats]=React.useState(THREATS0);
  const [run,setRun]=React.useState(null);const [freq,setFreq]=React.useState('Quotidienne');
  React.useEffect(()=>{if(!run)return;if(run.p>=100){const f=run.kind==='Rapide'?'12 408':'184 512';setScans(l=>[{id:Date.now(),when:"à l'instant",kind:run.kind,files:f,dur:run.kind==='Rapide'?'38 s':'6 min 14 s',found:0},...l]);setRun(null);flash('Analyse terminée · aucune menace');return}
    const t=setTimeout(()=>setRun(r=>({...r,p:Math.min(100,r.p+(r.kind==='Rapide'?9:4)),path:PATHS[Math.floor(r.p/100*PATHS.length)]||PATHS[PATHS.length-1]})),220);return()=>clearTimeout(t)},[run]);
  const start=k=>setRun({kind:k,p:0,path:PATHS[0]});
  const sel=(v,opts,f)=><span style={{position:'relative',display:'inline-flex',alignItems:'center'}}><select value={v} onChange={e=>f(e.target.value)} style={{height:28,padding:'0 28px 0 10px',borderRadius:8,border:'1px solid var(--border-default)',background:'var(--surface-app)',color:'var(--text-strong)',font:'500 12px/1 var(--font-sans)',outline:'none',appearance:'none',WebkitAppearance:'none',cursor:'pointer'}}>{opts.map(o=><option key={o} style={{background:'#141417'}}>{o}</option>)}</select><Icon name="chevron-down" size={12} color="var(--text-muted)" style={{position:'absolute',right:8,pointerEvents:'none'}}/></span>;
  const act=threats.filter(t=>t.st!=='deleted');
  return <div style={{display:'flex',flexDirection:'column',gap:14,paddingTop:4}}>
    <Back s={s} back={back}/>
    <div style={{...card,borderRadius:20,padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
      <Head ic="shield-check" title="Scanner de malwares" badge={!on?'Désactivé':run?'Analyse en cours':act.length?act.length+' en quarantaine':'Aucune menace'} tone={!on?'bad':act.length?'warn':'ok'} sub={s.name+' · ClamAV + règles YARA · signatures du jour'}>
        <Primary ic="scan-search" busy={!!run} onClick={()=>start('Complète')}>Analyse complète</Primary>
        <Button icon="zap" disabled={!!run} onClick={()=>start('Rapide')}>Rapide</Button>
      </Head>
      {run&&<div style={{display:'flex',flexDirection:'column',gap:10,padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)'}}>
        <div style={{display:'flex',alignItems:'center',gap:10}}><Icon name="loader" size={14} color="var(--text-muted)" style={{animation:'spin 1s linear infinite'}}/><span style={{flex:1,font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>Analyse {run.kind.toLowerCase()} · <span style={{fontFamily:'var(--font-mono)',color:'var(--text-strong)'}}>{run.path}</span></span><span style={{font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{run.p} %</span><button onClick={()=>{setRun(null);flash('Analyse annulée')}} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:'#a78bfa'}}>Annuler</button></div>
        <span style={{height:5,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden'}}><span style={{display:'block',height:'100%',width:run.p+'%',borderRadius:99,background:'#8b7cf6',transition:'width .2s'}}></span></span>
      </div>}
      <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
        <span style={{display:'flex',alignItems:'center',gap:10}}><Switch checked={on} onChange={v=>{setOn(v);flash(v?'Scanner activé':'Scanner désactivé')}}/><span style={{fontWeight:500,color:'var(--text-strong)'}}>Analyses planifiées</span></span>
        <span style={{display:'flex',alignItems:'center',gap:8}}>{sel(freq,['Quotidienne','Hebdomadaire','Toutes les 6 h'],v=>{setFreq(v);flash('Fréquence : '+v.toLowerCase())})}à 04:00</span>{sep}
        <span style={{display:'flex',alignItems:'center',gap:10}}><Switch checked={rt} onChange={v=>{setRt(v);flash(v?'Protection en temps réel activée':'Protection en temps réel désactivée')}}/>Temps réel sur /var/www et /tmp</span>
        <span style={{flex:1}}></span>
        <span style={lbl}>Signatures : <span style={{fontFamily:'var(--font-mono)',color:'var(--text-strong)'}}>8 712 044</span></span>
      </div>
    </div>
    {act.length>0&&<div style={card}>
      <div style={{padding:'16px 18px',font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>Menaces détectées</div>
      {act.map(t=><div key={t.id} style={{display:'flex',alignItems:'center',gap:14,padding:'13px 16px',borderTop:hair}}>
        <span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(248,113,113,.1)'}}><Icon name="bug" size={17} color="#f87171"/></span>
        <span style={{display:'flex',flexDirection:'column',gap:7,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{t.file}</span><span style={lbl}>{t.sig} · gravité {t.sev.toLowerCase()} · {t.when}</span></span>
        {pill(t.st==='quarantine'?'En quarantaine':'Restauré',t.st==='quarantine'?'warn':null)}
        <Button size="sm" onClick={()=>{setThreats(l=>l.map(x=>x.id===t.id?{...x,st:'deleted'}:x));flash('Fichier supprimé définitivement')}}>Supprimer</Button>
        <Button size="sm" variant="ghost" onClick={()=>{setThreats(l=>l.map(x=>x.id===t.id?{...x,st:'deleted'}:x));flash('Ajouté aux exceptions')}}>Ignorer</Button>
      </div>)}
    </div>}
    <div style={card}>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.4fr) minmax(0,1fr) 120px 110px 130px',gap:16,padding:'14px 16px',font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}><span>Analyse</span><span>Date</span><span>Fichiers</span><span>Durée</span><span>Résultat</span></div>
      {scans.map(sc=><div key={sc.id} style={{display:'grid',gridTemplateColumns:'minmax(0,1.4fr) minmax(0,1fr) 120px 110px 130px',gap:16,alignItems:'center',padding:'13px 16px',borderTop:hair}}>
        <span style={{display:'flex',alignItems:'center',gap:12}}><span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={sc.kind==='Rapide'?'zap':'scan-search'} size={17} color="var(--text-strong)"/></span><span style={{display:'flex',flexDirection:'column',gap:7}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Analyse {sc.kind.toLowerCase()}</span><span style={lbl}>{sc.kind==='Rapide'?'Dossiers sensibles':'Disque entier'}</span></span></span>
        <span>{pill(sc.when)}</span>
        <span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-body)'}}>{sc.files}</span>
        <span style={lbl}>{sc.dur}</span>
        <span style={{display:'flex',alignItems:'center',gap:7}}><Icon name={sc.found?'circle-alert':'circle-check'} size={15} color={sc.found?'#e5b454':'var(--green-500)'}/><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{sc.found?sc.found+' menace':'Sain'}</span></span>
      </div>)}
    </div>
  </div>;
}
Object.assign(window,{FirewallView,MalwareView});
})();
