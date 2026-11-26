250px #101010 app sidebar shell with top / nav / bottom slots.
```jsx
<Sidebar top={<><BrandMark/><WorkspaceSwitcher/></>} bottom={<UserMenu name="Alex Martin" email="alex@example.com"/>}>
  <NavSection label="Platform"><NavItem icon="layout-grid" label="Dashboard" active/></NavSection>
</Sidebar>
```
Pass `collapsed` to Sidebar and its children (NavSection, NavItem, WorkspaceSwitcher, UserMenu) for the 68px icon rail; toggle with `<IconButton variant="ghost" icon="panel-left"/>`.
