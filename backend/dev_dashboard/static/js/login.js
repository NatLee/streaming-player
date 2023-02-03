const getObjectKeys = (obj, prefix = '') => {
    return Object.entries(obj).reduce((collector, [key, val]) => {
      const newKeys = [ ...collector, prefix ? `${prefix}.${key}` : key ]
      if (Object.prototype.toString.call(val) === '[object Object]') {
        const newPrefix = prefix ? `${prefix}.${key}` : key
        const otherKeys = getObjectKeys(val, newPrefix)
        return [ ...newKeys, ...otherKeys ]
      }
      return newKeys
    }, [])
  }

$("form").on("submit", function (event) {
    event.preventDefault();

    $.ajax({
        type: "POST",
        url: "/api/auth/token",
        data: $(this).serialize(),
        success: function (data) {
            const keys = getObjectKeys(data);
            $.each(
                keys, function (idx) {
                    const key = keys[idx];
                    localStorage.setItem(key, data[key]);
                }
            )
            window.location.href = "/api/__hidden_dev_dashboard";
        }
    });
});