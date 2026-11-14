# Enable the optional integration against an explicitly supplied package before
# publishing it. The normal Home Manager profile keeps its FlakeHub pin.
{ homeFlake, writingPackage }:
(homeFlake.homeConfigurations.jone.extendModules {
  modules = [
    {
      programs.writing-lab = {
        enable = true;
        package = writingPackage;
      };
    }
  ];
}).config.home.activationPackage
