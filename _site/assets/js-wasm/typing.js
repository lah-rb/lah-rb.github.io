function typeMatchup(atk, def, motatk, motdef) {
//array, array, string, string
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
                    //possessor, consciense, spirit, duty, sacrifice, passion, service, satisfaction, survival
    const SpiritAtk = [10,0,0,0,0,0,0,0,0];
    const PossessorAtk = [0,10,0,0,0,0,0,0,0];
    const ConscienceAtk = [0,0,10,0,0,0,0,0,0];
    const SurvivalAtk = [0,0,0,10,0,0,0,0,0];
    const DutyAtk = [0,0,0,0,10,0,0,0,0];
    const SacrificeAtk = [0,0,0,0,0,10,0,0,0];
    const PassionAtk = [0,0,0,0,0,0,10,0,0];
    const ServiceAtk = [0,0,0,0,0,0,0,10,0];
    const SatisfactionAtk = [0,0,0,0,0,0,0,0,10];


    const motiveAtkMap = new Map();
    motiveAtkMap.set("Spirit", SpiritAtk);
    motiveAtkMap.set("Possessor", PossessorAtk);
    motiveAtkMap.set("Conscience", ConscienceAtk);
    motiveAtkMap.set("Survival", SurvivalAtk);
    motiveAtkMap.set("Duty", DutyAtk);
    motiveAtkMap.set("Sacrifice", SacrificeAtk);
    motiveAtkMap.set("Passion", PassionAtk);
    motiveAtkMap.set("Service", ServiceAtk);
    motiveAtkMap.set("Satisfaction", SatisfactionAtk);

    const PossessorDef =      0;
    const ConscienceDef =     1;
    const SpiritDef  =        2;
    const DutyDef =           3;
    const SacrificeDef =      4;
    const PassionDef =        5;
    const ServiceDef =        6;
    const SatisfactionDef =   7;
    const SurvivalDef =       8;

    const motiveDefMap = new Map();
    motiveDefMap.set("Possessor", PossessorDef);
    motiveDefMap.set("Conscience", ConscienceDef);
    motiveDefMap.set("Spirit", SpiritDef);
    motiveDefMap.set("Duty", DutyDef);
    motiveDefMap.set("Sacrifice", SacrificeDef);
    motiveDefMap.set("Passion", PassionDef);
    motiveDefMap.set("Service", ServiceDef);
    motiveDefMap.set("Satisfaction", SatisfactionDef);
    motiveDefMap.set("Survival", SurvivalDef);

    const SocietalPreservation = ['Spirit', 'Duty', 'Service']
    const SelfPreservation = ['Possessor','Survival', 'Satisfaction']
    const SupportPreservation = ['Conscience', 'Sacrifice', 'Passion']

    var hits = [];
    var societalMod = '';
    var selfMod = '';
    var supportMod = '';

    for (var i = 0; i < atk.length; i++) {
        for (var j = 0; j < def.length; j++) {
            hits.push(defMap.get(def[j])[atkMap.get(atk[i])]);
        }
    }
    if (motatk && motdef && motatk.trim() !== "" && motdef.trim() !== "") {
        hits.push(motiveAtkMap.get(motatk)[motiveDefMap.get(motdef)]);
        if (SocietalPreservation.includes(motatk)) {
            societalMod = '\nDefender must win 2 of 3'
        } 
        if (SelfPreservation.includes(motdef)) {
            selfMod = '\nDefender trys retreat before attack.'
        }
        if (SupportPreservation.includes(motdef)) {
            supportMod = '\nAtacker must win 2 of 3'
        }
    }
    var modifier = 0;

    for (var i = 0; i < hits.length; i++) {
        modifier += hits[i];
    }
    
    try {
        result = modifier + societalMod + selfMod + supportMod;
        return result;
    } catch (error) {
        return 'N/A';
    }
    

}