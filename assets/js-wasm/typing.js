function typeMatchup(atk, def) {

    const CenozoicDef = [0,3,1,1,-1,2,2,-1,-1,2,-3,-1,-2,1,-3];
    const DecrepitDef = [-3,0,1,2,1,-1,-3,-2,3,1,2,-1,-1,-1,2];
    const AngelicDef = [-1,-1,-3,3,2,-3,-2,1,1,1,-1,2,1,-1,-1];
    const BrutalDef = [-1,-2,-3,3,2,-1,-2,1,2,-1,-1,2,1,1,1];
    const ArborealDef = [1,-1,-2,-2,0,-3,-1,-2,2,1,3,1,-1,3,1];
    const AstralDef = [-2,1,3,1,3,0,-1,1,-1,-3,-2,-1,2,-2,1];
    const TelekineticDef = [-2,3,2,2,1,1,0,-3,-3,-1,-1,-1,1,-1,2];
    const GlitchDef = [1,2,-1,-1,2,-1,3,0,-3,-3,2,1,-2,1,-1];
    const MagicDef = [1,-3,-1,-2,-2,1,3,3,0,-1,2,1,1,-2,-1];
    const EndothermicDef = [-2,-1,-1,1,-1,3,1,3,1,0,1,2,-2,-3,-2];
    const AvianDef = [3,-2,1,1,-3,2,1,-2,-2,-1,0,3,-1,1,-1];
    const MechanicalDef = [1,1,-2,-2,-1,1,1,-1,-1,-2,-3,0,3,2,3];
    const AlgorithmicDef = [2,1,-1,-1,1,-2,-1,2,-1,2,1,-3,0,3,-3];
    const EnergeticDef = [-1,1,1,-1,-3,2,1,-1,2,3,-1,-2,-3,0,2];
    const EntropicDef = [3,-2,1,-1,-1,-1,-2,1,1,2,1,-3,3,-2,0];

    const defMap = new Map();
    defMap.set("Cenozoic", CenozoicDef);
    defMap.set("Decrepit", DecrepitDef);
    defMap.set("Angelic", AngelicDef);
    defMap.set("Brutal", BrutalDef);
    defMap.set("Arboreal", ArborealDef);
    defMap.set("Astral", AstralDef);
    defMap.set("Telekinetic", TelekineticDef);
    defMap.set("Glitch", GlitchDef);
    defMap.set("Magic", MagicDef);
    defMap.set("Endothermic", EndothermicDef);
    defMap.set("Avian", AvianDef);
    defMap.set("Mechanical", MechanicalDef);
    defMap.set("Algorithmic", AlgorithmicDef);
    defMap.set("Energetic", EnergeticDef);
    defMap.set("Entropic", EntropicDef);


    const CenozoicAtk =     0;
    const DecrepitAtk =     1;
    const AngelicAtk  =     2;
    const BrutalAtk =       3;
    const ArborealAtk =     4;
    const AstralAtk =       5;
    const TelekineticAtk =  6;
    const GlitchAtk =       7;
    const MagicAtk =        8;
    const EndothermicAtk =  9;
    const AvianAtk =        10;
    const MechanicalAtk =   11;
    const AlgorithmicAtk =  12;
    const EnergeticAtk =    13;
    const EntropicAtk =     14;

    const atkMap = new Map();
    atkMap.set("Cenozoic", CenozoicAtk);
    atkMap.set("Decrepit", DecrepitAtk);
    atkMap.set("Angelic", AngelicAtk);
    atkMap.set("Brutal", BrutalAtk);
    atkMap.set("Arboreal", ArborealAtk);
    atkMap.set("Astral", AstralAtk);
    atkMap.set("Telekinetic", TelekineticAtk);
    atkMap.set("Glitch", GlitchAtk);
    atkMap.set("Magic", MagicAtk);
    atkMap.set("Endothermic", EndothermicAtk);
    atkMap.set("Avian", AvianAtk);
    atkMap.set("Mechanical", MechanicalAtk);
    atkMap.set("Algorithmic", AlgorithmicAtk);
    atkMap.set("Energetic", EnergeticAtk);
    atkMap.set("Entropic", EntropicAtk);

    var hits = [];

    for (var i = 0; i < atk.length; i++) {
        for (var j = 0; j < def.length; j++) {
            hits.push(defMap.get(def[j])[atkMap.get(atk[i])]);
        }
    }

    var modifier = 0;

    for (var i = 0; i < hits.length; i++) {
        modifier += hits[i];
    }
    
    try {
        return modifier;
    } catch (error) {
        return 'N/A';
    }
    

}