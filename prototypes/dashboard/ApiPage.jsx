(()=>{
const {Button,IconButton,Icon,Modal}=window.CipheraDesignSystem;
const V='#8b6cff',hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const card={borderRadius:18,border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const IMG=k=>'../../web/src/assets/images/ssh-icons/'+k+'.png';
const TOOLS=[['Agents',[['claude','Claude Code'],['codex','Codex'],['grok','Grok'],['kimi','Kimi']]],['Terminal',[['cli','Ciphera CLI']]]];
const Logo=({k,s=28})=>k==='cli'?<span style={{width:s,height:s,borderRadius:8,display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name="terminal" size={s*.5}/></span>:<img src={IMG(k)} alt="" style={{width:s,height:s,borderRadius:8,objectFit:'cover',background:'#fff'}}/>;
const POL=[['plus','Commencer de zéro','Politique personnalisée','Custom'],['shield-check','Accès complet','Toutes les permissions du compte'],['eye','Lecture seule','Toutes les ressources · 48 autorisations'],['pencil','Écriture','Projets, déploiements · 31 autorisations'],['rocket','Déployer','Déclencher et annuler des déploiements'],['globe','DNS','Domaines et zones Cloudflare']];
function Tools({flash}){
  const [t,setT]=React.useState('claude');const [m,setM]=React.useState('auto');const [q,setQ]=React.useState('');const [msgs,setMsgs]=React.useState([]);
  const name=TOOLS.flatMap(g=>g[1]).find(x=>x[0]===t)[1];
  const cmd=t==='cli'?'curl -fsSL https://get.ciphera.ch/install.sh | sh':'npx -y @ciphera/mcp add --client '+t;
  const send=()=>{if(!q.trim())return;setMsgs(x=>[...x,['u',q],['a','Je déploie ciphera-web sur srv-par-01… ✓ build 42 s, en ligne sur app.ciphera.com']]);setQ('')};
  return <div style={{...card,display:'grid',gridTemplateColumns:'230px minmax(0,1fr)',overflow:'hidden'}}>
    <div style={{padding:14,borderRight:hair,display:'flex',flexDirection:'column',gap:14}}>
      {TOOLS.map(([g,it])=><div key={g} style={{display:'flex',flexDirection:'column',gap:2}}><span style={{...lbl,padding:'4px 10px 8px'}}>{g}</span>
        {it.map(([k,n])=>{const on=t===k;return <button key={k} onClick={()=>{setT(k);setMsgs([])}} style={{display:'flex',alignItems:'center',gap:10,height:40,padding:'0 10px',borderRadius:10,border:0,cursor:'pointer',background:on?'rgba(139,108,255,.12)':'transparent',color:on?'#b3a1ff':'var(--text-body)',font:'500 14px/1 var(--font-sans)'}}><Logo k={k} s={24}/>{n}</button>})}</div>)}
    </div>
    <div style={{display:'flex',flexDirection:'column'}}>
      <div style={{display:'flex',alignItems:'center',gap:12,padding:'16px 22px',borderBottom:hair}}><Logo k={t} s={34}/><span style={{flex:1,font:'500 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>Connecter {name}</span>
        <div style={{display:'flex',gap:2,padding:3,borderRadius:10,background:'var(--surface-raised)'}}>{[['auto','Installation en un clic'],['man','Configuration manuelle']].map(([k,l])=><button key={k} onClick={()=>setM(k)} style={{height:30,padding:'0 12px',borderRadius:8,border:0,cursor:'pointer',background:m===k?'var(--surface-selected)':'transparent',color:m===k?'var(--text-strong)':'var(--text-muted)',font:'500 12px/1 var(--font-sans)'}}>{l}</button>)}</div></div>
      <div style={{padding:22,display:'flex',flexDirection:'column',gap:22}}>
        {m==='auto'?<div style={{display:'flex',alignItems:'center',gap:20}}>
          <div style={{flex:1,display:'flex',flexDirection:'column',gap:8}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Installer</span><span style={{font:'400 13px/1.4 var(--font-sans)',color:'var(--text-muted)'}}>Ouvre {name} avec une invite qui installe le serveur MCP Ciphera.</span><span style={{display:'flex',gap:14,...lbl}}><span style={{display:'flex',alignItems:'center',gap:5}}><Icon name="lock" size={12}/>Aucune clé API requise</span><span style={{display:'flex',alignItems:'center',gap:5}}><Icon name="monitor" size={12}/>macOS, Windows et Linux</span></span></div>
          <button onClick={()=>flash(name+' ouvert · extension en cours d\'installation')} style={{display:'flex',alignItems:'center',gap:8,height:48,padding:'0 22px',borderRadius:12,border:0,cursor:'pointer',background:V,color:'#fff',font:'600 14px/1 var(--font-sans)',boxShadow:'0 8px 24px rgba(139,108,255,.35)'}}>Ouvrir {name} et installer<Icon name="external-link" size={14}/></button>
        </div>:<div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Exécutez dans votre terminal</span><div style={{display:'flex',alignItems:'center',gap:8,height:44,padding:'0 6px 0 14px',borderRadius:10,background:'var(--gray-0)',border:'1px solid var(--border-default)'}}><span style={{color:V,fontFamily:'var(--font-mono)'}}>$</span><span style={{flex:1,font:'400 13px/1 var(--font-mono)',color:'var(--text-body)'}}>{cmd}</span><IconButton size={30} variant="ghost" icon="copy" label="Copier" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(cmd);flash('Commande copiée')}}/></div></div>}
        <div style={{height:1,background:'var(--border-subtle)'}}></div>
        <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Ce que vous pouvez faire</span><span style={{font:'400 13px/1.4 var(--font-sans)',color:'var(--text-muted)'}}>Approuvez la connexion une seule fois, puis parlez à votre agent en termes clairs.</span></div>
        <div style={{borderRadius:14,border:'1px solid var(--border-default)',overflow:'hidden'}}>
          <div style={{display:'flex',alignItems:'center',justifyContent:'space-between',padding:'10px 14px',borderBottom:hair}}><span style={{display:'flex',alignItems:'center',gap:8,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}><Logo k={t} s={18}/>{name}</span><span style={{height:20,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:'var(--surface-selected)',...lbl,fontSize:11}}>Exemple</span></div>
          <div style={{minHeight:130,padding:14,display:'flex',flexDirection:'column',gap:8,background:'var(--surface-raised)'}}>
            {!msgs.length&&<div style={{display:'flex',flexWrap:'wrap',gap:6}}>{['Déploie ciphera-web en production','Ajoute api.ciphera.com sur Cloudflare','Pourquoi billing-api a échoué ?'].map(x=><button key={x} onClick={()=>setQ(x)} style={{height:30,padding:'0 12px',borderRadius:99,border:'1px solid var(--border-strong)',background:'transparent',cursor:'pointer',color:'var(--text-body)',font:'400 12px/1 var(--font-sans)'}}>{x}</button>)}</div>}
            {msgs.map(([w,x],i)=><span key={i} style={{alignSelf:w==='u'?'flex-end':'flex-start',maxWidth:'80%',padding:'9px 12px',borderRadius:12,background:w==='u'?V:'var(--surface-selected)',color:w==='u'?'#fff':'var(--text-body)',font:'400 13px/1.4 var(--font-sans)'}}>{x}</span>)}
          </div>
          <div style={{display:'flex',alignItems:'center',gap:8,padding:'8px 8px 8px 14px',borderTop:hair}}><input value={q} onChange={e=>setQ(e.target.value)} onKeyDown={e=>e.key==='Enter'&&send()} placeholder="Posez toutes vos questions à votre agent" style={{flex:1,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/><button onClick={send} style={{width:30,height:30,borderRadius:99,border:0,cursor:'pointer',background:V,color:'#fff',display:'flex',alignItems:'center',justifyContent:'center'}}><Icon name="arrow-up" size={15}/></button></div>
        </div>
      </div>
    </div>
  </div>;
}
const Sec=({t,children})=><div style={{...card,overflow:'hidden'}}><div style={{display:'flex',alignItems:'center',gap:6,padding:'12px 18px',borderBottom:hair,background:'var(--surface-raised)',font:'500 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>{t}<Icon name="info" size={12}/></div><div style={{padding:14}}>{children}</div></div>;
const Step=({n,t,d,last,children})=><div style={{display:'grid',gridTemplateColumns:'36px minmax(0,1fr)',gap:16}}>
    <div style={{display:'flex',flexDirection:'column',alignItems:'center'}}><span style={{width:30,height:30,borderRadius:99,display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(180deg,rgba(139,108,255,.35),rgba(139,108,255,.12))',border:'1px solid rgba(139,108,255,.45)',color:'#d6ccff',font:'600 12px/1 var(--font-mono)',boxShadow:'0 0 0 4px rgba(139,108,255,.06)'}}>{n}</span>{!last&&<span style={{flex:1,width:1,marginTop:6,background:'linear-gradient(180deg,rgba(139,108,255,.35),var(--border-subtle))'}}></span>}</div>
    <div style={{display:'flex',flexDirection:'column',gap:14,paddingBottom:last?0:30}}><div style={{display:'flex',flexDirection:'column',gap:6,paddingTop:5}}><span style={{font:'600 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{font:'400 13px/1.4 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></div>{children}</div>
  </div>;
const CH='abcdefghijklmnopqrstuvwxyz0123456789';
function TokenPreview({name,pol,until,noExp,ipn,SC,LV}){
  const ref=React.useRef(null);const [tilt,setTilt]=React.useState([0,0,50,50]);
  const [txt,setTxt]=React.useState('');const key=name+pol+until;
  React.useEffect(()=>{let f=0;const target='cph_'+[...(name||'x')].reduce((a,c)=>a+c.charCodeAt(0),pol.length*7).toString(36).padEnd(6,'k')+'••••••••••••';const t=setInterval(()=>{f++;setTxt(target.split('').map((c,i)=>i<f*1.6||c==='•'&&i<f*1.6?c:c==='_'?c:CH[Math.random()*36|0]).join(''));if(f*1.6>target.length)clearInterval(t)},28);return()=>clearInterval(t)},[key]);
  const move=e=>{const r=ref.current.getBoundingClientRect();const x=(e.clientX-r.left)/r.width,y=(e.clientY-r.top)/r.height;setTilt([(0.5-y)*10,(x-0.5)*14,x*100,y*100])};
  const lvTxt=['Aucun','Lecture','Écriture','Admin'];
  return <div style={{display:'flex',flexDirection:'column',gap:14}}>
    <div style={{perspective:900}}>
      <div ref={ref} onMouseMove={move} onMouseLeave={()=>setTilt([0,0,50,50])} style={{position:'relative',aspectRatio:'1.586',borderRadius:18,overflow:'hidden',transform:'rotateX('+tilt[0]+'deg) rotateY('+tilt[1]+'deg)',transition:'transform .25s cubic-bezier(.2,.8,.2,1)',background:'linear-gradient(160deg,#232326 0%,#18181b 50%,#111113 100%)',border:'1px solid rgba(255,255,255,.1)',boxShadow:'0 24px 48px -24px rgba(0,0,0,.8), inset 0 1px 0 rgba(255,255,255,.06)'}}>
        <div style={{position:'absolute',inset:0,backgroundImage:'radial-gradient(rgba(255,255,255,.035) 1px,transparent 1px)',backgroundSize:'14px 14px',maskImage:'linear-gradient(135deg,transparent 30%,#000 100%)',WebkitMaskImage:'linear-gradient(135deg,transparent 30%,#000 100%)'}}></div>
        <div style={{position:'relative',height:'100%',padding:'18px 20px',display:'flex',flexDirection:'column'}}>
          <div style={{display:'flex',alignItems:'center',justifyContent:'space-between'}}>
            <img src="../../web/src/assets/brand/logo.svg" alt="Ciphera" style={{height:11,opacity:.85}}/>
            <span style={{display:'flex',alignItems:'center',gap:6,height:22,padding:'0 9px',borderRadius:99,background:'rgba(255,255,255,.06)',border:'1px solid rgba(255,255,255,.08)',font:'500 11px/1 var(--font-sans)',color:'var(--text-body)'}}><span className="live-ping" style={{width:6,height:6,borderRadius:99,background:'var(--green-500)'}}></span>Brouillon</span>
          </div>
          <div style={{marginTop:22,width:38,height:28,borderRadius:6,background:'linear-gradient(135deg,#e8e4d8,#b8b2a2 45%,#d9d4c6 70%,#9f9a8c)',boxShadow:'inset 0 0 0 1px rgba(0,0,0,.2)',position:'relative',overflow:'hidden'}}>{[9,19].map(y=><span key={y} style={{position:'absolute',left:0,right:0,top:y,height:1,background:'rgba(0,0,0,.25)'}}></span>)}<span style={{position:'absolute',top:0,bottom:0,left:18,width:1,background:'rgba(0,0,0,.25)'}}></span></div>
          <span style={{flex:1}}></span>
          <span style={{font:'500 13px/1 var(--font-mono)',letterSpacing:'.06em',color:'var(--text-muted)'}}>{txt}</span>
          <div style={{display:'flex',alignItems:'flex-end',justifyContent:'space-between',gap:12,marginTop:12}}>
            <div style={{display:'flex',flexDirection:'column',gap:5,minWidth:0}}><span style={{font:'500 9px/1 var(--font-sans)',letterSpacing:'.1em',textTransform:'uppercase',color:'var(--text-subtle)'}}>Nom</span><span style={{font:'600 14px/1 var(--font-mono)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{name||'sans-nom'}</span></div>
            <div style={{display:'flex',flexDirection:'column',gap:5,alignItems:'flex-end',flex:'none'}}><span style={{font:'500 9px/1 var(--font-sans)',letterSpacing:'.1em',textTransform:'uppercase',color:'var(--text-subtle)'}}>Expire</span><span style={{font:'500 12px/1 var(--font-mono)',color:noExp?'#e5b454':'var(--text-strong)'}}>{noExp?'∞':until}</span></div>
          </div>
        </div>
      </div>
    </div>
    <div style={{...card,overflow:'hidden'}}>
      <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',borderBottom:hair}}>{[['Politique',pol],['Restriction IP',ipn?ipn+' règle'+(ipn>1?'s':''):'Toutes les IP']].map(([k,v],i)=><div key={k} style={{display:'flex',flexDirection:'column',gap:7,padding:'13px 16px',borderLeft:i?hair:'none'}}><span style={{...lbl,fontSize:11}}>{k}</span><span key={v} className="cph-pop" style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span></div>)}</div>
      <div style={{padding:'12px 16px 14px',display:'flex',flexDirection:'column',gap:10}}>
        {SC.map(([t,ic],i)=><div key={t} style={{display:'grid',gridTemplateColumns:'16px minmax(0,1fr) 92px 56px',gap:10,alignItems:'center'}}>
          <Icon name={ic} size={13} color={LV[i]?'var(--text-muted)':'var(--text-faint)'}/>
          <span style={{font:'400 12px/1 var(--font-sans)',color:LV[i]?'var(--text-body)':'var(--text-faint)'}}>{t}</span>
          <span style={{position:'relative',height:6,borderRadius:99,background:'var(--viz-4)',overflow:'hidden'}}><span style={{position:'absolute',left:0,top:0,bottom:0,width:(LV[i]/3*100)+'%',borderRadius:99,background:LV[i]===3?'linear-gradient(90deg,#7a5cf0,#b3a1ff)':LV[i]===2?'#8b6cff':'#5b4a99',transition:'width .55s cubic-bezier(.2,.8,.2,1) '+(i*45)+'ms'}}></span></span>
          <span style={{textAlign:'right',font:'500 11px/1 var(--font-sans)',color:LV[i]===3?'#c4b7ff':LV[i]?'var(--text-muted)':'var(--text-faint)'}}>{lvTxt[LV[i]]}</span>
        </div>)}
      </div>
    </div>
  </div>;
}
function Tokens({flash,create,setCreate}){
  const [list,setList]=React.useState([]);const [name,setName]=React.useState('proud-river-7c21');const [pol,setPol]=React.useState(1);const [exp,setExp]=React.useState('90 jours');const [ips,setIps]=React.useState([['Autoriser','']]);const [tok,setTok]=React.useState(null);
  const SC=[['Projets','layers'],['Déploiements','rocket'],['Serveurs','server'],['Domaines & DNS','globe'],['Variables','braces'],['Logs','scroll-text']];
  const LV=[[1,1,1,1,1,1],[3,3,3,3,3,3],[1,1,1,1,0,1],[2,2,1,1,2,1],[1,2,0,0,0,1],[0,0,0,2,0,0]][pol];
  const LN=['—','Lecture','Écriture','Admin'];
  const days={"Pas d'expiration":0,'7 jours':7,'30 jours':30,'90 jours':90,'1 an':365,'Personnalisé':60}[exp];
  const until=days?new Date(Date.now()+days*864e5).toLocaleDateString('fr-FR',{day:'numeric',month:'short',year:'numeric'}):'Jamais';
  const ipn=ips.filter(r=>r[1].trim()).length;
  const rnd=()=>{const a=['proud','silent','amber','swift','lunar','bold','quiet'],b=['river','falcon','cedar','orbit','pixel','harbor'];setName(a[Math.random()*7|0]+'-'+b[Math.random()*6|0]+'-'+Math.random().toString(16).slice(2,6))};
  const go=()=>{const v='cph_'+Math.random().toString(36).slice(2,12)+Math.random().toString(36).slice(2,14);setList(l=>[{name,pol:POL[pol][1],exp,created:"à l'instant"},...l]);setTok(v);setCreate(false)};
  const inp={height:40,padding:'0 12px',borderRadius:10,border:'1px solid var(--border-strong)',background:'var(--surface-control)',color:'var(--text-strong)',outline:'none',boxShadow:'inset 0 1px 0 rgba(255,255,255,.03)'};
  if(create)return <div style={{display:'flex',flexDirection:'column',gap:22}}>
    <div style={{display:'flex',alignItems:'center',gap:12}}><button onClick={()=>setCreate(false)} style={{display:'flex',alignItems:'center',gap:6,padding:0,border:0,background:'transparent',cursor:'pointer',color:'var(--text-muted)',font:'500 13px/1 var(--font-sans)'}}><Icon name="arrow-left" size={14}/>Jetons API</button><span style={{color:'var(--text-faint)'}}>/</span><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Nouveau jeton</span></div>
    <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) 340px',gap:28,alignItems:'start'}}>
      <div style={{...card,padding:'26px 26px 28px'}}>
        <Step n="01" t="Nom du jeton" d="Pour le reconnaître dans les logs d'audit.">
          <div style={{display:'flex',gap:8,maxWidth:460}}><div style={{...inp,flex:1,display:'flex',alignItems:'center',gap:10}}><Icon name="key-round" size={14} color={V}/><input value={name} onChange={e=>setName(e.target.value)} style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'500 14px/1 var(--font-mono)'}}/></div><IconButton size={40} icon="dices" label="Générer un nom" onClick={rnd}/></div>
        </Step>
        <Step n="02" t="Politique d'autorisation" d="Les permissions disponibles dépendent de votre rôle dans l'organisation.">
          <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:8}}>{POL.map(([ic,t,d,tag],i)=>{const on=pol===i;return <button key={t} onClick={()=>setPol(i)} style={{position:'relative',display:'flex',flexDirection:'column',gap:12,padding:14,borderRadius:14,cursor:'pointer',textAlign:'left',border:'1px solid '+(on?'rgba(139,108,255,.6)':'var(--border-default)'),background:on?'linear-gradient(180deg,rgba(139,108,255,.14),rgba(139,108,255,.03))':'var(--surface-raised)',boxShadow:on?'0 8px 24px -8px rgba(139,108,255,.35)':'none',transition:'all .15s'}}>
            <span style={{position:'absolute',top:12,right:12,width:16,height:16,borderRadius:99,border:'1.5px solid '+(on?V:'var(--gray-500)'),background:on?V:'transparent',display:'flex',alignItems:'center',justifyContent:'center'}}>{on&&<Icon name="check" size={10} color="#fff"/>}</span>
            <span style={{width:36,height:36,borderRadius:10,display:'flex',alignItems:'center',justifyContent:'center',background:on?V:'var(--surface-selected)',boxShadow:on?'inset 0 1px 0 rgba(255,255,255,.3)':'none'}}><Icon name={ic} size={16} color={on?'#fff':'var(--text-body)'}/></span>
            <span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{display:'flex',alignItems:'center',gap:6,font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{t}{tag&&<span style={{height:17,display:'flex',alignItems:'center',padding:'0 6px',borderRadius:99,background:'rgba(139,108,255,.18)',color:'#c4b7ff',fontSize:10}}>{tag}</span>}</span><span style={{font:'400 11px/1.4 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></span>
          </button>})}</div>
        </Step>
        <Step n="03" t="Expiration" d={days?'Le jeton cessera de fonctionner le '+until+'.':'Recommandé uniquement pour les serveurs internes.'}>
          <div style={{display:'flex',gap:3,padding:4,borderRadius:12,background:'var(--surface-raised)',border:'1px solid var(--border-default)',alignSelf:'flex-start'}}>{["Pas d'expiration",'7 jours','30 jours','90 jours','1 an','Personnalisé'].map(x=>{const on=exp===x;return <button key={x} onClick={()=>setExp(x)} style={{height:32,padding:'0 13px',borderRadius:9,cursor:'pointer',border:0,background:on?V:'transparent',color:on?'#fff':'var(--text-muted)',font:'500 12px/1 var(--font-sans)',boxShadow:on?'0 4px 12px rgba(139,108,255,.35), inset 0 1px 0 rgba(255,255,255,.25)':'none'}}>{x}</button>})}</div>
          {!days&&<span style={{display:'flex',alignItems:'center',gap:8,font:'400 12px/1 var(--font-sans)',color:'#e5b454'}}><Icon name="triangle-alert" size={13}/>Un jeton sans expiration est plus risqué en cas de fuite.</span>}
        </Step>
        <Step n="04" t="Filtrage des adresses IP" d="Facultatif. Laissez vide pour autoriser toutes les adresses." last>
          <div style={{display:'flex',flexDirection:'column',gap:8}}>{ips.map((r,i)=>{const al=r[0]==='Autoriser';return <div key={i} style={{display:'flex',gap:8}}>
            <button onClick={()=>setIps(x=>x.map((y,j)=>j===i?[al?'Refuser':'Autoriser',y[1]]:y))} style={{display:'flex',alignItems:'center',gap:6,height:40,padding:'0 12px',borderRadius:10,cursor:'pointer',border:'1px solid '+(al?'rgba(34,192,138,.35)':'rgba(239,68,68,.35)'),background:al?'rgba(34,192,138,.08)':'rgba(239,68,68,.08)',color:al?'var(--green-500)':'#f87171',font:'500 12px/1 var(--font-sans)',minWidth:112}}><Icon name={al?'check':'ban'} size={13}/>{r[0]}<Icon name="chevrons-up-down" size={11}/></button>
            <input value={r[1]} onChange={e=>setIps(x=>x.map((y,j)=>j===i?[y[0],e.target.value]:y))} placeholder="203.0.113.0/24" style={{...inp,flex:1,font:'400 13px/1 var(--font-mono)'}}/>
            {ips.length>1&&<IconButton size={40} variant="ghost" icon="x" label="Retirer" onClick={()=>setIps(x=>x.filter((_,j)=>j!==i))}/>}
          </div>})}
          <button onClick={()=>setIps(x=>[...x,['Autoriser','']])} style={{alignSelf:'flex-start',display:'flex',alignItems:'center',gap:6,height:30,padding:'0 10px',borderRadius:8,border:'1px dashed var(--border-strong)',background:'transparent',cursor:'pointer',color:'var(--text-body)',font:'500 12px/1 var(--font-sans)'}}><Icon name="plus" size={13}/>Ajouter une règle</button></div>
        </Step>
      </div>
      <div style={{position:'sticky',top:16,display:'flex',flexDirection:'column',gap:12}}>
        <TokenPreview name={name} pol={POL[pol][1]} until={until} noExp={!days} ipn={ipn} SC={SC} LV={LV}/>
        <Button variant="inverse" disabled={!name.trim()} onClick={go}>Créer le jeton</Button>
        <Button onClick={()=>setCreate(false)}>Annuler</Button>
      </div>
    </div>
  </div>;
  return <div style={{display:'flex',flexDirection:'column',gap:14}}>
    {!list.length?<div style={{...card,display:'flex',flexDirection:'column',alignItems:'center',textAlign:'center',gap:12,padding:'56px 24px'}}><span style={{font:'600 22px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Créez votre premier jeton API</span><span style={{maxWidth:440,font:'400 14px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>Les jetons API authentifient l'accès à votre compte Ciphera, pour vos scripts, votre CI et vos outils tiers.</span><button onClick={()=>setCreate(true)} style={{marginTop:8,height:38,padding:'0 18px',borderRadius:10,border:0,cursor:'pointer',background:V,color:'#fff',font:'600 13px/1 var(--font-sans)'}}>Créer un jeton</button></div>
    :<div style={{...card,overflow:'hidden'}}>{list.map((k,i)=><div key={i} style={{display:'flex',alignItems:'center',gap:14,padding:'14px 18px',borderTop:i?hair:'none'}}><span style={{width:34,height:34,borderRadius:9,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(139,108,255,.14)'}}><Icon name="key-round" size={15} color={V}/></span><span style={{flex:1,display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 14px/1 var(--font-mono)',color:'var(--text-strong)'}}>{k.name}</span><span style={lbl}>{k.pol} · expire : {k.exp} · créé {k.created}</span></span><IconButton size={30} variant="ghost" icon="trash-2" label="Révoquer" onClick={()=>{setList(l=>l.filter((_,j)=>j!==i));flash('Jeton révoqué')}}/></div>)}</div>}
    {tok&&<Modal title="Jeton créé" description="Copiez-le maintenant : il ne sera plus affiché." onClose={()=>setTok(null)} width={520} footer={<Button variant="inverse" onClick={()=>setTok(null)}>Terminé</Button>}><div style={{display:'flex',alignItems:'center',gap:8,height:44,padding:'0 6px 0 14px',borderRadius:10,background:'var(--gray-0)',border:'1px solid rgba(139,108,255,.4)'}}><span style={{flex:1,font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{tok}</span><IconButton size={30} variant="ghost" icon="copy" label="Copier" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(tok);flash('Jeton copié')}}/></div></Modal>}
  </div>;
}
function ApiPage(){
  const tab='tokens';const [create,setCreate]=React.useState(false);const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2200)};
  return <div style={{padding:'0 32px 48px 34px',display:'flex',flexDirection:'column',gap:18}}>
    <div style={{display:'flex',alignItems:'center',gap:12}}><span style={{flex:1,font:'400 14px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>Créez des jetons pour accéder à l'API Ciphera depuis vos scripts, votre CI et vos outils.</span><Button iconRight="external-link" onClick={()=>flash('docs.example.com/api')}>Documentation</Button>{tab==='tokens'&&!create&&<button onClick={()=>setCreate(true)} style={{height:36,padding:'0 16px',borderRadius:'var(--radius-md)',border:0,cursor:'pointer',background:V,color:'#fff',font:'600 13px/1 var(--font-sans)'}}>Créer un jeton</button>}</div>
    <Tokens flash={flash} create={create} setCreate={setCreate}/>
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.ApiPage=ApiPage;
})();
