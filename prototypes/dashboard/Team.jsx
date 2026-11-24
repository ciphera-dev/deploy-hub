(()=>{
const {Button,IconButton,Badge,Icon,StatusDot,Modal,TextField,Tabs,Switch,RadioCard,Select}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const PROJECTS=['ciphera-web','billing-api','dashboard-v2','analytics','marketing-site'];
const RES=[['projects','Projets','layers'],['deploy','Déploiements','rocket'],['servers','Serveurs','server'],['db','Bases de données','database'],['domains','Domaines','globe'],['vars','Variables et secrets','braces'],['security','Sécurité','shield'],['logs','Logs','scroll-text'],['team','Équipe','users'],['billing','Facturation','credit-card']];
const LEVELS=['Aucun','Lecture','Écriture','Admin'];
const full=l=>Object.fromEntries(RES.map(r=>[r[0],l]));
const ROLES=[
  {id:'owner',name:'Propriétaire',desc:'Contrôle total, y compris la facturation et la suppression de l\'organisation.',perms:full(3),locked:true},
  {id:'admin',name:'Administrateur',desc:'Gère l\'infrastructure et les membres, sans accès à la facturation.',perms:{...full(3),billing:1}},
  {id:'dev',name:'Développeur',desc:'Déploie et configure les projets auxquels il a accès.',perms:{projects:2,deploy:2,servers:1,db:1,domains:2,vars:2,security:0,logs:1,team:0,billing:0}},
  {id:'ops',name:'Ops',desc:'Exploite les serveurs, la sécurité et les bases de données.',perms:{projects:1,deploy:2,servers:3,db:2,domains:2,vars:1,security:3,logs:1,team:0,billing:0}},
  {id:'viewer',name:'Lecteur',desc:'Consulte les tableaux de bord et les logs, sans rien modifier.',perms:{projects:1,deploy:1,servers:1,db:1,domains:1,vars:0,security:1,logs:1,team:0,billing:0}},
];
const U=(o)=>({mfa:true,status:'active',projects:'all',...o});
const INIT=[
  U({id:1,name:'Alex Martin',email:'alex@ciphera.com',role:'owner',last:'maintenant',since:'mars 2024'}),
  U({id:2,name:'Lina Moreau',email:'lina@ciphera.com',role:'admin',last:'il y a 3 min',since:'avr. 2024'}),
  U({id:3,name:'Yanis Benali',email:'yanis@ciphera.com',role:'dev',last:'il y a 1 h',since:'juin 2024',projects:['ciphera-web','dashboard-v2','analytics']}),
  U({id:4,name:'Camille Roux',email:'camille@ciphera.com',role:'ops',last:'il y a 4 h',since:'sept. 2024'}),
  U({id:5,name:'Hugo Lefèvre',email:'hugo@ciphera.com',role:'dev',last:'hier',since:'janv. 2025',mfa:false,projects:['billing-api']}),
  U({id:6,name:'Inès Garnier',email:'ines@agence-nord.fr',role:'viewer',last:'il y a 6 j',since:'août 2025',projects:['marketing-site'],guest:true}),
  U({id:7,name:'Thomas Petit',email:'thomas@ciphera.com',role:'dev',last:'il y a 2 mois',since:'févr. 2025',status:'suspended',projects:['ciphera-web']}),
  U({id:8,name:'',email:'nora@ciphera.com',role:'dev',last:'—',since:'',status:'invited',mfa:false,sent:'il y a 2 j',projects:['ciphera-web']}),
];
const AUDIT=[['Lina Moreau','a changé le rôle de','Camille Roux','Développeur → Ops','il y a 2 h'],['Alex Martin','a invité','nora@ciphera.com','Développeur · ciphera-web','il y a 2 j'],['Alex Martin','a suspendu','Thomas Petit','Inactif depuis 60 j','il y a 5 j'],['Lina Moreau','a révoqué une session de','Hugo Lefèvre','Chrome · Lyon','il y a 1 sem.'],['Alex Martin','a modifié le rôle','Ops','Sécurité : Écriture → Admin','il y a 2 sem.']];
const initials=n=>(n||'?').split(/[ @.]/).filter(Boolean).slice(0,2).map(x=>x[0].toUpperCase()).join('');
const hue=s=>[...s].reduce((a,c)=>a+c.charCodeAt(0),0)%360;
function Avatar({u,size=32}){return <span style={{width:size,height:size,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',font:`600 ${size*.36}px/1 var(--font-sans)`,color:u.status==='invited'?'var(--text-subtle)':'#0b0b0b',background:u.status==='invited'?'transparent':`oklch(0.82 0.06 ${hue(u.email)})`,border:u.status==='invited'?'1px dashed var(--gray-500)':'none'}}>{u.status==='invited'?<Icon name="mail" size={size*.42}/>:initials(u.name)}</span>}
const roleName=id=>(ROLES.find(r=>r.id===id)||{}).name;
const lvColor=['var(--viz-4)','var(--viz-3)','var(--viz-2)','var(--viz-1)'];
function LevelPill({l}){return <span style={{display:'inline-flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:l?'var(--text-body)':'var(--text-faint)'}}><span style={{display:'flex',gap:2}}>{[1,2,3].map(i=><span key={i} style={{width:4,height:10,borderRadius:1,background:i<=l?lvColor[3]:'var(--viz-4)'}}></span>)}</span>{LEVELS[l]}</span>}
function Menu({items,onClose}){
  React.useEffect(()=>{const f=()=>onClose();setTimeout(()=>document.addEventListener('click',f),0);return()=>document.removeEventListener('click',f)},[]);
  return <div style={{position:'absolute',top:32,right:0,zIndex:30,width:230,padding:4,background:'var(--surface-tooltip)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-tooltip)'}}>
    {items.filter(Boolean).map((it,i)=>it==='-'?<div key={i} style={{height:1,background:'var(--border-strong)',margin:'4px 0'}}></div>:<button key={i} onClick={it[2]} onMouseEnter={e=>e.currentTarget.style.background='var(--surface-selected)'} onMouseLeave={e=>e.currentTarget.style.background='transparent'} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'8px 10px',border:0,borderRadius:6,background:'transparent',cursor:'pointer',textAlign:'left',color:it[3]?'#f87171':'var(--text-body)',font:'400 13px/1 var(--font-sans)'}}><Icon name={it[0]} size={14}/>{it[1]}</button>)}
  </div>;
}
function ProjChips({value,onChange}){
  const all=value==='all';
  const chip=(on,t,f)=><button key={t} onClick={f} style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:99,cursor:'pointer',font:'500 12px/1 var(--font-mono)',border:'1px solid '+(on?'var(--gray-500)':'var(--border-strong)'),background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)'}}>{on&&<Icon name="check" size={11}/>}{t}</button>;
  return <div style={{display:'flex',flexWrap:'wrap',gap:6}}>
    {chip(all,'Tous les projets',()=>onChange(all?[]:'all'))}
    {PROJECTS.map(p=>{const on=all||value.includes(p);return chip(on,p,()=>onChange(all?PROJECTS.filter(x=>x!==p):on?value.filter(x=>x!==p):[...value,p].length===PROJECTS.length?'all':[...value,p]))})}
  </div>;
}
function Label({children,hint}){return <div style={{display:'flex',alignItems:'baseline',justifyContent:'space-between'}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{children}</span>{hint&&<span style={{...lbl,fontSize:11}}>{hint}</span>}</div>}

function UserModal({user,onClose,onSave}){
  const edit=!!user;
  const [name,setName]=React.useState(user?.name||'');const [email,setEmail]=React.useState(user?.email||'');
  const [role,setRole]=React.useState(user?.role||'dev');const [projects,setProjects]=React.useState(user?.projects||'all');
  const [method,setMethod]=React.useState('invite');const [mfa,setMfa]=React.useState(user?user.mfa:true);const [pwd]=React.useState(()=>Math.random().toString(36).slice(2,8)+'-'+Math.random().toString(36).slice(2,8).toUpperCase());
  const [expires,setExpires]=React.useState(false);
  const valid=/^[^@\s]+@[^@\s]+\.[a-z]{2,}$/i.test(email)&&(method==='invite'||name);
  const R=ROLES.find(r=>r.id===role);
  return <Modal title={edit?'Modifier '+(user.name||user.email):'Créer un utilisateur'} description={edit?'Rôle, périmètre et sécurité du compte.':'Le compte aura accès à la plateforme selon le rôle choisi.'} onClose={onClose} width={680}
    footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" icon={edit?'check':method==='invite'?'send':'user-plus'} disabled={!valid} onClick={()=>onSave({name,email:email.trim().toLowerCase(),role,projects,mfa,method})}>{edit?'Enregistrer':method==='invite'?'Envoyer l\'invitation':'Créer le compte'}</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:22}}>
      {!edit&&<div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:8}}>
        <RadioCard icon="send" title="Invitation par e-mail" description="L'utilisateur choisit son mot de passe" selected={method==='invite'} onClick={()=>setMethod('invite')}/>
        <RadioCard icon="key-round" title="Mot de passe temporaire" description="À changer à la première connexion" selected={method==='pwd'} onClick={()=>setMethod('pwd')}/>
      </div>}
      <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:12}}>
        <TextField label={method==='invite'&&!edit?'Nom (facultatif)':'Nom complet'} placeholder="Nora Kaci" value={name} onChange={setName}/>
        <TextField label="E-mail" placeholder="nora@ciphera.com" value={email} onChange={setEmail}/>
      </div>
      {method==='pwd'&&!edit&&<div style={{display:'flex',alignItems:'center',gap:10,padding:'10px 12px',borderRadius:'var(--radius-md)',border:'1px solid var(--border-default)',background:'var(--gray-0)'}}><Icon name="lock" size={13} color="var(--text-subtle)"/><span style={{flex:1,font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)'}}>{pwd}</span><IconButton size={28} variant="ghost" icon="copy" label="Copier" onClick={()=>navigator.clipboard&&navigator.clipboard.writeText(pwd)}/></div>}
      <div style={{display:'flex',flexDirection:'column',gap:10}}>
        <Label hint={user?.role==='owner'?'Le propriétaire ne peut pas être rétrogradé':null}>Rôle</Label>
        <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))',gap:6}}>
          {ROLES.filter(r=>r.id!=='owner'||user?.role==='owner').map(r=>{const on=role===r.id;return <button key={r.id} disabled={user?.role==='owner'} onClick={()=>setRole(r.id)} style={{height:36,borderRadius:'var(--radius-md)',cursor:user?.role==='owner'?'default':'pointer',font:'500 12px/1 var(--font-sans)',border:'1px solid '+(on?'var(--gray-500)':'var(--border-strong)'),background:on?'var(--surface-selected)':'var(--surface-control)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{r.name}</button>})}
        </div>
        <div style={{display:'grid',gridTemplateColumns:'repeat(5,minmax(0,1fr))',gap:1,borderRadius:'var(--radius-lg)',overflow:'hidden',border:'1px solid var(--border-default)',background:'var(--border-subtle)'}}>
          {RES.map(([k,t,ic])=><div key={k} style={{display:'flex',flexDirection:'column',gap:8,padding:'10px 12px',background:'var(--surface-app)'}}><span style={{display:'flex',alignItems:'center',gap:6,...lbl,fontSize:11,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}><Icon name={ic} size={11}/>{t}</span><LevelPill l={R.perms[k]}/></div>)}
        </div>
      </div>
      {role!=='owner'&&role!=='admin'&&<div style={{display:'flex',flexDirection:'column',gap:10}}><Label hint={projects==='all'?'Inclut les futurs projets':projects.length+' sélectionné(s)'}>Projets accessibles</Label><ProjChips value={projects} onChange={setProjects}/></div>}
      <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)'}}>
        <div style={{padding:'12px 14px'}}><Switch checked={mfa} onChange={setMfa} label="Exiger la double authentification" description="Application TOTP ou clé de sécurité (WebAuthn) obligatoire à la connexion."/></div>
        {!edit&&<div style={{padding:'12px 14px',borderTop:hair}}><Switch checked={expires} onChange={setExpires} label="Accès temporaire" description="Le compte est suspendu automatiquement après 30 jours."/></div>}
      </div>
    </div>
  </Modal>;
}

const RC={owner:['#e5b454','crown'],admin:['#a78bfa','shield'],dev:['#7dd3fc','code'],ops:['#86efac','server'],viewer:['#a1a1aa','eye']};
const rgbOf=h=>{const n=parseInt(h.slice(1),16);return (n>>16&255)+','+(n>>8&255)+','+(n&255)};
function RoleBadge({id,muted,chev,open}){const [c,ic]=RC[id]||['#a1a1aa','user'];const r=rgbOf(c);return <span style={{display:'inline-flex',alignItems:'center',gap:6,height:26,padding:'0 9px 0 8px',borderRadius:99,background:'rgba('+r+','+(open?.18:.10)+')',border:'1px solid rgba('+r+','+(open?.45:.25)+')',font:'500 12px/1 var(--font-sans)',color:muted?'var(--text-muted)':c,whiteSpace:'nowrap',transition:'all .15s'}}><Icon name={ic} size={12} color={c}/>{roleName(id)}{chev&&<Icon name="chevron-down" size={12} color={c} style={{transform:open?'rotate(180deg)':'none',transition:'transform .15s'}}/>}</span>}
function RolePicker({value,onChange}){
  const [open,setOpen]=React.useState(false);const ref=React.useRef();const pop=React.useRef();const [pos,setPos]=React.useState({top:0,left:0});
  React.useEffect(()=>{if(!open)return;const b=ref.current.getBoundingClientRect();setPos({top:b.bottom+6,left:b.left});const f=e=>{if(!ref.current.contains(e.target)&&!(pop.current&&pop.current.contains(e.target)))setOpen(false)};const k=e=>e.key==='Escape'&&setOpen(false);document.addEventListener('mousedown',f);document.addEventListener('keydown',k);window.addEventListener('scroll',()=>setOpen(false),{once:true,capture:true});return()=>{document.removeEventListener('mousedown',f);document.removeEventListener('keydown',k)}},[open]);
  return <span ref={ref} style={{display:'inline-flex'}}><button onClick={()=>setOpen(o=>!o)} style={{padding:0,border:0,background:'transparent',cursor:'pointer'}}><RoleBadge id={value} chev open={open}/></button>
    {open&&ReactDOM.createPortal(<div ref={pop} style={{position:'fixed',top:pos.top,left:pos.left,zIndex:1000,width:280,padding:6,borderRadius:14,background:'#141417',border:'1px solid rgba(255,255,255,.1)',boxShadow:'0 20px 50px -12px rgba(0,0,0,.8)',animation:'notifIn .15s ease-out'}}>
      <span style={{display:'block',padding:'8px 10px 6px',font:'600 10px/1 var(--font-sans)',letterSpacing:'.08em',textTransform:'uppercase',color:'var(--text-subtle)'}}>Changer le rôle</span>
      {ROLES.filter(r=>r.id!=='owner').map(r=>{const on=r.id===value;const [c,ic]=RC[r.id];const rr=rgbOf(c);return <button key={r.id} onClick={()=>{if(!on)onChange(r.id);setOpen(false)}} onMouseEnter={e=>e.currentTarget.style.background='rgba(255,255,255,.05)'} onMouseLeave={e=>e.currentTarget.style.background=on?'rgba(255,255,255,.04)':'transparent'} style={{display:'flex',alignItems:'flex-start',gap:10,width:'100%',padding:'9px 10px',borderRadius:10,border:0,cursor:'pointer',textAlign:'left',background:on?'rgba(255,255,255,.04)':'transparent'}}>
        <span style={{width:28,height:28,borderRadius:8,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'rgba('+rr+',.12)',border:'1px solid rgba('+rr+',.25)'}}><Icon name={ic} size={13} color={c}/></span>
        <span style={{display:'flex',flexDirection:'column',gap:5,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{r.name}</span><span style={{font:'400 11px/1.4 var(--font-sans)',color:'var(--text-subtle)'}}>{r.desc}</span></span>
        {on&&<Icon name="check" size={14} color={c} style={{marginTop:7}}/>}
      </button>})}
    </div>,document.body)}
  </span>;
}
function MembersTab({users,setUsers,flash,openUser}){
  const [q,setQ]=React.useState('');const [f,setF]=React.useState('Tous');const [menu,setMenu]=React.useState(null);const [confirm,setConfirm]=React.useState(null);const [h,setH]=React.useState(null);
  const owners=users.filter(u=>u.role==='owner').length;
  const list=users.filter(u=>(f==='Tous'||(f==='Actifs'&&u.status==='active')||(f==='Invités'&&u.status==='invited')||(f==='Suspendus'&&u.status==='suspended')||(f==='Sans 2FA'&&!u.mfa&&u.status==='active'))&&(u.name+u.email).toLowerCase().includes(q.toLowerCase()));
  const upd=(id,o)=>setUsers(us=>us.map(u=>u.id===id?{...u,...o}:u));
  const COLS='minmax(0,1.6fr) 150px minmax(0,1.2fr) 96px 120px 40px';
  return <div style={{display:'flex',flexDirection:'column',gap:14}}>
    <div style={{display:'flex',alignItems:'center',gap:10}}>
      <div style={{display:'flex',alignItems:'center',gap:8,height:32,padding:'0 10px',width:260,borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher un membre" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-sans)'}}/></div>
      <div style={{display:'flex',gap:2,padding:2,borderRadius:9,border:'1px solid var(--border-default)',background:'var(--surface-control)'}}>{['Tous','Actifs','Invités','Suspendus','Sans 2FA'].map(x=><button key={x} onClick={()=>setF(x)} style={{height:28,padding:'0 10px',border:0,borderRadius:7,cursor:'pointer',background:f===x?'var(--surface-selected)':'transparent',color:f===x?'var(--text-strong)':'var(--text-muted)',font:'500 12px/1 var(--font-sans)'}}>{x}</button>)}</div>
      <span style={{flex:1}}></span>
      <Button icon="download" onClick={()=>{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob(['nom,email,role,statut,2fa\n'+users.map(u=>[u.name,u.email,roleName(u.role),u.status,u.mfa?'oui':'non'].join(',')).join('\n')],{type:'text/csv'}));a.download='equipe.csv';document.body.appendChild(a);a.click();a.remove();flash('Export CSV téléchargé')}}>Exporter</Button>
    </div>
    <div style={{...card}}>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:16,padding:'12px 18px',...lbl}}><span>Membre</span><span>Rôle</span><span>Projets</span><span>2FA</span><span>Activité</span><span></span></div>
      {list.map(u=>{const inv=u.status==='invited',sus=u.status==='suspended';return <div key={u.id} onMouseEnter={()=>setH(u.id)} onMouseLeave={()=>setH(null)} style={{display:'grid',gridTemplateColumns:COLS,gap:16,alignItems:'center',padding:'12px 18px',borderTop:hair,background:h===u.id?'var(--surface-raised)':'transparent'}}>
        <div style={{display:'flex',alignItems:'center',gap:12,minWidth:0,opacity:sus?.5:1}}>
          <Avatar u={u}/>
          <div style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}>
            <span style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:inv?'var(--text-muted)':'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{inv?'Invitation envoyée':u.name}</span>{u.id===1&&<Badge>Vous</Badge>}{u.guest&&<Badge>Externe</Badge>}{sus&&<Badge tone="negative">Suspendu</Badge>}</span>
            <span style={{...lbl,whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{u.email}</span>
          </div>
        </div>
        {u.role==='owner'||sus?<RoleBadge id={u.role} muted={sus}/>
          :<RolePicker value={u.role} onChange={id=>{upd(u.id,{role:id});flash((u.name||u.email)+' → '+roleName(id))}}/>}
        <div style={{display:'flex',gap:4,flexWrap:'wrap',minWidth:0}}>{u.projects==='all'||u.role==='owner'||u.role==='admin'?<span style={{...lbl,color:'var(--text-body)'}}>Tous les projets</span>:u.projects.slice(0,2).map(p=><span key={p} style={{height:20,display:'flex',alignItems:'center',padding:'0 6px',borderRadius:5,background:'var(--surface-selected)',font:'400 11px/1 var(--font-mono)',color:'var(--text-body)'}}>{p}</span>)}{u.projects!=='all'&&u.role!=='owner'&&u.role!=='admin'&&u.projects.length>2&&<span style={{...lbl,fontSize:11,alignSelf:'center'}}>+{u.projects.length-2}</span>}</div>
        <span>{inv?<span style={lbl}>—</span>:u.mfa?<span style={{display:'flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name="shield-check" size={13} color="var(--green-500)"/>Activée</span>:<span style={{display:'flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:'#e5b454'}}><Icon name="shield-alert" size={13}/>Inactive</span>}</span>
        <span style={{display:'flex',flexDirection:'column',gap:5}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}>{inv?'Envoyée '+u.sent:u.last}</span>{!inv&&<span style={{...lbl,fontSize:11}}>Depuis {u.since}</span>}</span>
        <div style={{position:'relative',justifySelf:'end'}}>
          <IconButton size={28} variant="ghost" icon="ellipsis" label="Actions" onClick={()=>setMenu(m=>m===u.id?null:u.id)}/>
          {menu===u.id&&<Menu onClose={()=>setMenu(null)} items={inv?[['send','Renvoyer l\'invitation',()=>{upd(u.id,{sent:"à l'instant"});flash('Invitation renvoyée à '+u.email)}],['link','Copier le lien d\'invitation',()=>flash('Lien copié · expire dans 7 j')],'-',['x','Annuler l\'invitation',()=>{setUsers(us=>us.filter(x=>x.id!==u.id));flash('Invitation annulée')},true]]
            :[['pencil','Modifier les accès',()=>openUser(u)],u.id!==1&&['key-round','Réinitialiser le mot de passe',()=>flash('Lien de réinitialisation envoyé à '+u.email)],!u.mfa&&['shield-check','Exiger la 2FA',()=>{upd(u.id,{mfa:true});flash('2FA exigée à la prochaine connexion')}],u.id!==1&&['log-out','Révoquer les sessions',()=>flash('Sessions de '+u.name+' révoquées')],u.role!=='owner'&&'-',u.role!=='owner'&&(sus?['user-check','Réactiver',()=>{upd(u.id,{status:'active'});flash(u.name+' réactivé')}]:['user-x','Suspendre',()=>{upd(u.id,{status:'suspended'});flash(u.name+' suspendu')},true]),u.role!=='owner'&&['trash-2','Supprimer le compte',()=>setConfirm(u),true]]}/>}
        </div>
      </div>})}
      {!list.length&&<div style={{padding:'28px 18px',borderTop:hair,...lbl}}>Aucun membre ne correspond.</div>}
    </div>
    {confirm&&<Modal title={'Supprimer '+confirm.name+' ?'} description="Le compte est supprimé définitivement, ses sessions et clés API sont révoquées. Ses déploiements restent dans l'historique." onClose={()=>setConfirm(null)} width={460} footer={<><Button onClick={()=>setConfirm(null)}>Annuler</Button><Button variant="inverse" icon="trash-2" onClick={()=>{setUsers(us=>us.filter(x=>x.id!==confirm.id));flash('Compte supprimé');setConfirm(null)}}>Supprimer</Button></>}></Modal>}
  </div>;
}

function RolesTab({users,roles,setRoles,flash}){
  const [sel,setSel]=React.useState('dev');const R=roles.find(r=>r.id===sel);
  const set=(k,l)=>{if(R.locked)return;setRoles(rs=>rs.map(r=>r.id===sel?{...r,perms:{...r.perms,[k]:l}}:r))};
  return <div style={{display:'grid',gridTemplateColumns:'280px minmax(0,1fr)',gap:20,alignItems:'start'}}>
    <div style={{display:'flex',flexDirection:'column',gap:4}}>
      {roles.map(r=>{const on=r.id===sel,n=users.filter(u=>u.role===r.id).length;return <button key={r.id} onClick={()=>setSel(r.id)} style={{display:'flex',flexDirection:'column',gap:8,padding:'12px 14px',borderRadius:'var(--radius-lg)',textAlign:'left',cursor:'pointer',border:'1px solid '+(on?'var(--border-strong)':'transparent'),background:on?'var(--surface-raised)':'transparent'}}>
        <span style={{display:'flex',alignItems:'center',gap:8,font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{r.locked&&<Icon name="lock" size={11} color="var(--text-subtle)"/>}{r.name}<span style={{flex:1}}></span><span style={{display:'flex'}}>{users.filter(u=>u.role===r.id).slice(0,3).map((u,i)=><span key={u.id} style={{marginLeft:i?-6:0,borderRadius:99,boxShadow:'0 0 0 2px var(--surface-app)'}}><Avatar u={u} size={20}/></span>)}</span><span style={{...lbl,fontSize:11,fontFamily:'var(--font-mono)',width:14,textAlign:'right'}}>{n}</span></span>
        <span style={{font:'400 12px/1.45 var(--font-sans)',color:'var(--text-subtle)',textWrap:'pretty'}}>{r.desc}</span>
      </button>})}
      <Button variant="ghost" icon="plus" onClick={()=>{const id='custom'+Date.now();setRoles(rs=>[...rs,{id,name:'Rôle personnalisé',desc:'Copie du rôle Lecteur, à ajuster.',perms:{...rs.find(r=>r.id==='viewer').perms}}]);setSel(id);flash('Rôle créé')}}>Créer un rôle</Button>
    </div>
    <div style={{...card,overflow:'hidden'}}>
      <div style={{display:'flex',alignItems:'center',gap:12,padding:'16px 18px'}}>
        <span style={{font:'500 15px/1 var(--font-sans)',color:'var(--text-strong)'}}>{R.name}</span>{R.locked&&<Badge>Non modifiable</Badge>}
        <span style={{flex:1}}></span><span style={lbl}>{users.filter(u=>u.role===R.id).length} membre(s)</span>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) repeat(4,92px)',padding:'10px 18px',borderTop:hair,...lbl}}><span>Ressource</span>{LEVELS.map(l=><span key={l} style={{textAlign:'center'}}>{l}</span>)}</div>
      {RES.map(([k,t,ic])=><div key={k} style={{display:'grid',gridTemplateColumns:'minmax(0,1fr) repeat(4,92px)',alignItems:'center',minHeight:46,padding:'0 18px',borderTop:hair}}>
        <span style={{display:'flex',alignItems:'center',gap:10,font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name={ic} size={14} color="var(--text-muted)"/>{t}</span>
        {LEVELS.map((l,i)=>{const on=R.perms[k]===i;return <span key={l} style={{display:'flex',justifyContent:'center'}}><button onClick={()=>set(k,i)} disabled={R.locked} style={{width:18,height:18,borderRadius:99,padding:0,cursor:R.locked?'default':'pointer',border:'1.5px solid '+(on?'var(--text-strong)':'var(--gray-500)'),background:'transparent',display:'flex',alignItems:'center',justifyContent:'center',opacity:R.locked&&!on?.4:1}}>{on&&<span style={{width:8,height:8,borderRadius:99,background:'var(--text-strong)'}}></span>}</button></span>})}
      </div>)}
      <div style={{display:'flex',alignItems:'center',gap:8,padding:'12px 18px',borderTop:hair,font:'400 11px/1.5 var(--font-sans)',color:'var(--text-faint)'}}><Icon name="info" size={12}/>Lecture : consulter · Écriture : créer, modifier, déployer · Admin : supprimer et gérer les accès</div>
    </div>
  </div>;
}

function AuditTab(){
  return <div style={{...card,overflow:'hidden'}}>
    {AUDIT.map(([who,act,target,detail,when],i)=><div key={i} style={{display:'grid',gridTemplateColumns:'28px minmax(0,1fr) 110px',gap:14,alignItems:'center',padding:'14px 18px',borderTop:i?hair:'none'}}>
      <Avatar u={{name:who,email:who,status:'active'}} size={28}/>
      <div style={{display:'flex',flexDirection:'column',gap:6,minWidth:0}}><span style={{font:'400 13px/1.3 var(--font-sans)',color:'var(--text-muted)'}}><span style={{color:'var(--text-strong)',fontWeight:500}}>{who}</span> {act} <span style={{color:'var(--text-strong)',fontWeight:500}}>{target}</span></span><span style={{...lbl,fontSize:11}}>{detail}</span></div>
      <span style={{...lbl,textAlign:'right'}}>{when}</span>
    </div>)}
  </div>;
}

function TeamPage({addOpen,closeAdd}){
  const [users,setUsers]=React.useState(INIT);const [roles,setRoles]=React.useState(ROLES);const [tab,setTab]=React.useState('members');const [edit,setEdit]=React.useState(null);
  const [toast,setToast]=React.useState('');const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2200)};
  const active=users.filter(u=>u.status==='active'),inv=users.filter(u=>u.status==='invited'),noMfa=active.filter(u=>!u.mfa);
  const save=d=>{if(edit){setUsers(us=>us.map(u=>u.id===edit.id?{...u,...d,name:d.name||u.name}:u));flash('Accès de '+(d.name||d.email)+' mis à jour');setEdit(null);return}
    setUsers(us=>[...us,U({id:Date.now(),...d,status:d.method==='invite'?'invited':'active',sent:"à l'instant",last:d.method==='invite'?'—':'jamais',since:'nov. 2026',mfa:d.mfa})]);
    flash(d.method==='invite'?'Invitation envoyée à '+d.email:'Compte '+d.email+' créé');closeAdd&&closeAdd()};
  const SUM=[['Membres actifs',active.length,'sur 10 sièges inclus'],['Administrateurs',users.filter(u=>u.role==='owner'||u.role==='admin').length,'Propriétaire inclus'],['Invitations',inv.length,inv.length?'en attente de réponse':'aucune en attente'],['Double authentification',Math.round((active.length-noMfa.length)/active.length*100)+' %',noMfa.length?noMfa.length+' membre sans 2FA':'tous protégés',noMfa.length>0]];
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:18}}>
    {(()=>{const sep=<span style={{width:1,height:14,background:'var(--border-strong)'}}></span>;const adm=users.filter(u=>u.role==='owner'||u.role==='admin').length;const pct=Math.round((active.length-noMfa.length)/active.length*100);
      return <div style={{...card,borderRadius:20,padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
      <div style={{display:'flex',alignItems:'center',gap:16}}>
        <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18)'}}><Icon name="users" size={24} color="#f2f2f2"/></span>
        <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>Équipe Ciphera</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:noMfa.length?'rgba(229,180,84,.12)':'rgba(34,197,94,.12)',font:'500 12px/1 var(--font-sans)',color:noMfa.length?'#e5b454':'var(--green-500)'}}>{noMfa.length?noMfa.length+' sans 2FA':'Tous protégés'}</span></span><span style={lbl}>{active.length} membres actifs sur 10 sièges inclus</span></span>
        <span style={{display:'flex'}}>{active.slice(0,5).map((u,n)=><span key={u.id} style={{marginLeft:n?-8:0,borderRadius:99,boxShadow:'0 0 0 3px var(--surface-app)'}}><Avatar u={u} size={32}/></span>)}</span>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:18,flexWrap:'wrap',padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}>
        <span>Administrateurs : <span style={{fontWeight:600,color:'var(--text-strong)'}}>{adm}</span></span>{sep}
        <span>Invitations : <span style={{fontWeight:600,color:'var(--text-strong)'}}>{inv.length}</span></span>{sep}
        <span>Double authentification : <span style={{fontWeight:600,color:pct===100?'var(--green-500)':'#e5b454'}}>{pct} %</span></span>
        <span style={{flex:1}}></span>
        {noMfa.length>0&&<button onClick={()=>{setUsers(us=>us.map(u=>({...u,mfa:u.status==='invited'?u.mfa:true})));flash('2FA obligatoire pour toute l\'organisation')}} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:'#a78bfa'}}>Rendre la 2FA obligatoire</button>}
      </div>
      <div style={{display:'grid',gridTemplateColumns:'repeat(3,minmax(0,1fr))',gap:12}}>
        {[['Sièges utilisés',active.length,'/ 10',active.length*10],['Comptes protégés par 2FA',active.length-noMfa.length,'/ '+active.length,pct],['Invitations en attente',inv.length,'envoyées',inv.length*20]].map(([k,v,u,b])=><div key={k} style={{display:'flex',flexDirection:'column',gap:14,padding:'18px 20px',borderRadius:14,border:'1px solid var(--border-default)'}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{k}</span><span style={{display:'flex',alignItems:'flex-end',gap:12}}><span style={{display:'flex',alignItems:'baseline',gap:5,flex:1}}><span style={{font:'600 24px/1 var(--font-sans)',color:'var(--text-strong)'}}>{v}</span><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-muted)'}}>{u}</span></span><span style={{width:112,height:8,borderRadius:99,background:'var(--surface-selected)',overflow:'hidden',marginBottom:6}}><span style={{display:'block',height:'100%',width:Math.min(100,b)+'%',borderRadius:99,background:'#8b7cf6'}}></span></span></span></div>)}
      </div>
    </div>})()}
    <div style={{paddingLeft:12}}><Tabs tabs={[{value:'members',label:'Membres',count:users.length},{value:'roles',label:'Rôles et permissions',count:roles.length},{value:'audit',label:'Journal d\'accès'}]} value={tab} onChange={setTab}/></div>
    {tab==='members'&&<MembersTab users={users} setUsers={setUsers} flash={flash} openUser={setEdit}/>}
    {tab==='roles'&&<RolesTab users={users} roles={roles} setRoles={setRoles} flash={flash}/>}
    {tab==='audit'&&<AuditTab/>}
    {(addOpen||edit)&&<UserModal user={edit} onClose={()=>{setEdit(null);closeAdd&&closeAdd()}} onSave={save}/>}
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.TeamPage=TeamPage;
})();
