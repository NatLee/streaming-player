$("form").on("submit", function (event) {
    event.preventDefault();
    $.ajax({
        type: "POST",
        url: "/auth/users",
        data: $(this).serialize(),
        success: function () {
            window.location.href = "/api/__hidden_dev_dashboard";
        },
        error: function (data) {
            console.log(data.responseText)
        }
    });
});