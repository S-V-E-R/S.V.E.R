-- War map geography: each genre gets a fixed spot on the map (axial hex coordinates)
-- and each faction one capital. Neighbors are now the territories that touch on the map.
ALTER TABLE faction_genres ADD COLUMN map_q integer, ADD COLUMN map_r integer,
    ADD COLUMN capital boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT faction_genre_map CHECK ((map_q IS NULL) = (map_r IS NULL)),
    ADD CONSTRAINT faction_genre_capital CHECK (NOT capital OR home IS NOT NULL);
CREATE UNIQUE INDEX faction_genre_map_cell ON faction_genres(map_q,map_r) WHERE map_q IS NOT NULL;
CREATE UNIQUE INDEX faction_genre_capital_home ON faction_genres(home) WHERE capital;
UPDATE faction_genres g SET map_q=m.q,map_r=m.r,capital=m.capital FROM (VALUES
 ('art',1,0,false),('education_coding',2,0,false),('puzzle_simulation',0,1,false),
 ('strategy_4x',1,1,true),('rts_moba',2,1,false),('card_board',-1,2,false),
 ('music',4,0,false),('cozy_sandbox',5,0,false),('community_events',3,1,false),
 ('mmos_rpgs',4,1,true),('coop_party',2,2,false),
 ('crafting_making',0,2,false),('fighting',1,2,false),('speedrunning',0,3,false),
 ('fps_battle_royale',1,3,true),('sports_racing',-1,4,false)
) AS m(id,q,r,capital) WHERE g.id=m.id;
UPDATE faction_genres g SET neighbors=ARRAY(
    SELECT n.id FROM faction_genres n
    WHERE n.id<>g.id AND n.map_q IS NOT NULL
      AND abs(n.map_q-g.map_q)+abs(n.map_r-g.map_r)+abs(n.map_q+n.map_r-g.map_q-g.map_r)=2
    ORDER BY n.position,n.id)
WHERE g.map_q IS NOT NULL;
